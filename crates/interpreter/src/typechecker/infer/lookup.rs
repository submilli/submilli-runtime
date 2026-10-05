use std::collections::BTreeMap;

use crate::compiler_error::CompilerFailure;
use crate::{MethodSig, ObjectField, PropertySig, Span, Type, TypeKind, TypeSymbol};

use super::Inferer;
use super::generic::substitute_or_record;

/// What a write to one member's `field` would target.
///
/// The two halves are separate questions and the write paths need them apart: a
/// `readonly` field and a getter-only accessor still have a *shape*, and a
/// rejected write needs that shape to type its value even though no write to it
/// is permitted.
pub(super) struct FieldWrite {
    /// What a value written here would have to be — a setter's parameter type
    /// where there is one, the field's own type otherwise, or [`Type::Error`]
    /// where no value can be written at all (a getter with no setter), which is
    /// the shape `infer_assign_field` checks such a write against.
    pub(super) ty: Type,
    pub(super) writable: bool,
}

/// Why one member of a union receiver can't back a field read.
pub(super) enum MemberFieldMiss {
    /// The member declares nothing of that name.
    Absent,
    /// It declares it as a method. A union read resolves by field name at
    /// runtime; methods live in the vtable, not the field payload.
    Method,
    /// A host interface's property, reachable only through a getter import
    /// keyed to that one interface.
    HostDispatched,
}

impl<'a> Inferer<'a> {
    /// The local binding `name` resolves to, unless another `case` clause of an
    /// enclosing `switch` declares it, which hides every binding from outside.
    pub(super) fn visible_local(&self, name: &str) -> Option<&super::scopes::ScopeEntry> {
        if self.declaration_in_another_case_clause(name).is_some() {
            return None;
        }
        self.scopes.get(name)
    }

    /// Whether the module-level binding of `name` is visible here: no other
    /// `case` clause declares it, and it isn't a later declaration read directly
    /// at the top level. In a function body, a later declaration is bound early.
    pub(super) fn top_symbol_visible(
        &mut self,
        name: &str,
        span: Span,
    ) -> Result<bool, CompilerFailure> {
        if self.declaration_in_another_case_clause(name).is_some() {
            return Ok(false);
        }
        Ok(!self.hides_later_global(name, span)?)
    }

    /// Direct-call metadata belongs only to a function that survives lexical lookup.
    pub(super) fn lookup_top_function(&self, name: &str) -> Option<&super::ValueEntry> {
        if self.scopes.get(name).is_some()
            || self.declaration_in_another_case_clause(name).is_some()
        {
            return None;
        }
        self.top_symbols
            .get(name)
            .filter(|entry| matches!(entry.kind, crate::ValueKind::Function { .. }))
    }

    pub(super) fn lookup_named_type(&self, name: &str) -> Option<&TypeSymbol> {
        self.types.lookup(name)
    }

    /// Owning package of a source-resolved type name, for stamping onto the
    /// by-name [`Type`] this resolution produces. Read from the namespace entry
    /// (carried at insertion), never parsed from the mangled name. Defaults to
    /// the user package for a name that isn't in the namespace (callers only
    /// reach this after a successful [`lookup_named_type`](Self::lookup_named_type)).
    pub(super) fn type_package(&self, name: &str) -> crate::Package {
        crate::Package(
            self.types
                .package_of(name)
                .unwrap_or(crate::mangle::USER_PACKAGE)
                .to_string(),
        )
    }

    /// Resolve an interface/enum/alias symbol for *structural* access on an
    /// already-typed value — member access, cast-shape derivation, `for-of`
    /// dispatch. Keyed by the type's own `package`, so this never depends on
    /// what the current module imported. Falls back to the import-scoped
    /// [`TypeNamespace`](super::type_namespace::TypeNamespace) for the current
    /// module's own (user-declared) types, which live only there.
    pub(super) fn lookup_structural_type(
        &self,
        mangled: &crate::MangledName,
        name: &str,
    ) -> Option<&TypeSymbol> {
        self.type_registry
            .lookup(mangled)
            .or_else(|| self.types.lookup(name))
    }

    /// Bundle the import-scoped namespace + FQN registry for the assignability
    /// checker, so recursive-alias back-edges into a library type resolve by
    /// package rather than depending on the consumer's imports.
    pub(super) fn resolver(&self) -> super::assignable::TypeResolver<'_> {
        super::assignable::TypeResolver {
            types: &self.types,
            registry: &self.type_registry,
            limits: &self.type_limits,
        }
    }

    pub(super) fn find_method(
        &self,
        recv_ty: &Type,
        name: &str,
    ) -> Option<(
        MethodSig,
        BTreeMap<String, Type>,
        crate::MangledName,
        crate::Dispatch,
    )> {
        let array_view = recv_ty.array_like_union_view();
        let recv_ty = array_view.as_ref().unwrap_or(recv_ty);
        let (mangled, _package, interface_name, args) = recv_ty.interface_routing()?;
        let sym = self.lookup_structural_type(&mangled, interface_name)?;
        match &sym.kind {
            TypeKind::Interface {
                generics,
                methods,
                dispatch,
                ..
            } => {
                let Some(sig) = methods.get(name) else {
                    return self.undeclared_interface_to_string(sym, name, *dispatch);
                };
                let mut sig = sig.clone();
                if array_view.is_some() {
                    sig = restrict_to_reads(sig, generics)?;
                }
                let bindings: BTreeMap<String, Type> = generics.iter().cloned().zip(args).collect();
                Some((sig, bindings, sym.mangled_name.clone(), *dispatch))
            }
            TypeKind::Class { .. } => {
                let Some(resolved) = self.class_method_in_chain(&sym.mangled_name, &args, name)
                else {
                    // Universal vtable methods: every class instance answers
                    // `toString`/`toJson` through vtable slots 0/1 even without
                    // a declared method — the call compiles here and codegen
                    // routes it through `vtable_slot_for_method`.
                    if matches!(name, "toString" | "toJson") {
                        return Some((
                            MethodSig {
                                generics: Vec::new(),
                                params: Vec::new(),
                                ret: Type::String,
                                predicate: None,
                                doc: None,
                            },
                            BTreeMap::new(),
                            sym.mangled_name.clone(),
                            crate::Dispatch::VTable,
                        ));
                    }
                    return None;
                };
                // A private method of a class from another module is invisible —
                // resolve to nothing so the call site reports "no such method".
                if resolved.visibility == crate::Visibility::Private
                    && !self.local_class_mangles.contains(&resolved.declared_by)
                {
                    return None;
                }
                Some((
                    resolved.sig,
                    resolved.bindings,
                    sym.mangled_name.clone(),
                    crate::Dispatch::VTable,
                ))
            }
            _ => None,
        }
    }

    /// `toString` on a value of a program-declared interface that doesn't declare it.
    ///
    /// Every non-null value answers `toString` (spec §1.6). Such a value is an
    /// object, so the call dispatches as `Object`'s does, through vtable slot 0: the
    /// value's own `toString`, else `"[object Object]"`. An interface whose
    /// `toString` is a property keeps that property's rules, so an optional one
    /// still needs a guard. The runtime's own interfaces stay out: their host
    /// values would print `[object Object]` where JS names the class.
    fn undeclared_interface_to_string(
        &self,
        sym: &crate::TypeSymbol,
        name: &str,
        dispatch: crate::Dispatch,
    ) -> Option<(
        MethodSig,
        BTreeMap<String, Type>,
        crate::MangledName,
        crate::Dispatch,
    )> {
        let TypeKind::Interface { properties, .. } = &sym.kind else {
            return None;
        };
        let is_runtime_interface = sym.mangled_name.as_str().starts_with("submilli:");
        if name != "toString"
            || dispatch != crate::Dispatch::VTable
            || is_runtime_interface
            || properties.contains_key(name)
        {
            return None;
        }
        let object = Type::Object {
            fields: BTreeMap::new(),
            index: None,
        };
        self.find_method(&object, name)
    }

    /// Resolve an interface property regardless of dispatch kind. VTable
    /// interfaces read their properties through shape-based field dispatch
    /// (`FieldAccess`); Direct/Static interfaces through a prelude getter
    /// (`InterfacePropertyAccess`). `infer_field_access` branches on the
    /// returned dispatch to pick the right lowering.
    pub(super) fn lookup_interface_property(
        &self,
        recv_ty: &Type,
        name: &str,
    ) -> Option<(
        PropertySig,
        BTreeMap<String, Type>,
        crate::MangledName,
        crate::Dispatch,
    )> {
        let array_view = recv_ty.array_like_union_view();
        let recv_ty = array_view.as_ref().unwrap_or(recv_ty);
        let (mangled, _package, interface_name, args) = recv_ty.interface_routing()?;
        let sym = self.lookup_structural_type(&mangled, interface_name)?;
        let TypeKind::Interface {
            generics,
            properties,
            dispatch,
            ..
        } = &sym.kind
        else {
            return None;
        };
        let sig = properties.get(name)?.clone();
        let bindings: BTreeMap<String, Type> = generics.iter().cloned().zip(args).collect();
        Some((sig, bindings, sym.mangled_name.clone(), *dispatch))
    }

    /// Field read type for one member of a union receiver, resolved by the same
    /// authority the single-member receiver uses — the interface's *property*
    /// map, not its methods; `class_field_visible`, so module-scoped privacy
    /// still applies. Compare `narrow_scopes::narrow_source_field_ty`, which
    /// answers the same question for a *narrow source*: it accepts any
    /// interface dispatch and skips `null` members, because it is rebuilding a
    /// read the access site already accepted rather than deciding one.
    ///
    /// The `Err` carries which of the three rejections it was, produced at the
    /// site that knows. The caller turns it into a diagnostic naming the member;
    /// it must not fall back to a laxer lookup, or the read would compile to a
    /// shape scan for something the member doesn't carry.
    pub(super) fn union_member_field_read_ty(
        &self,
        member: &Type,
        field: &str,
    ) -> Result<Type, MemberFieldMiss> {
        if self.find_method(member, field).is_some() {
            return Err(MemberFieldMiss::Method);
        }
        match member.peel() {
            Type::Object { fields, index } => fields
                .get(field)
                .map(ObjectField::read_ty)
                .or_else(|| index.as_ref().map(crate::IndexSignature::read_ty))
                .ok_or(MemberFieldMiss::Absent),
            Type::InterfaceRef { .. } => {
                let (sig, bindings, _, dispatch) = self
                    .lookup_interface_property(member, field)
                    .ok_or(MemberFieldMiss::Absent)?;
                // Every user-declared interface is VTable-dispatched and reads
                // through the shape scan, which is what a union receiver can
                // emit. A Direct/Static one reads through a getter import keyed
                // by its own interface — a union has no single key.
                if dispatch != crate::Dispatch::VTable {
                    return Err(MemberFieldMiss::HostDispatched);
                }
                let ty = if bindings.is_empty() {
                    sig.ty.clone()
                } else {
                    substitute_or_record(&sig.ty, &bindings, &self.type_limits)
                };
                Ok(ObjectField::widen_optional(sig.optional, ty))
            }
            Type::ClassRef { mangled, args, .. } => {
                let (sig, _decl) = self
                    .class_field_visible(mangled, args, field)
                    .ok_or(MemberFieldMiss::Absent)?;
                Ok(ObjectField::widen_optional(sig.optional, sig.ty.clone()))
            }
            _ => Err(MemberFieldMiss::Absent),
        }
    }

    /// The field map of one union member, expanded through the type registry so
    /// that a named interface or class is described on the same terms as an
    /// inline object type. `None` for a member that carries no fields at all.
    ///
    /// The single authority for that question — every union member-access path
    /// (shape analysis, the read classifier, the write reporter) routes through
    /// here so none of them can disagree about what a member holds.
    pub(super) fn member_shape(&self, member: &Type) -> Option<BTreeMap<String, ObjectField>> {
        match member.peel() {
            Type::Object { fields, .. } => Some(fields.clone()),
            Type::InterfaceRef {
                mangled,
                name,
                args,
                ..
            }
            | Type::ClassRef {
                mangled,
                name,
                args,
                ..
            } => self.structural_form(mangled, name, args),
            _ => None,
        }
    }

    /// Member shapes of a union, expanded through the type registry so that a
    /// named interface or class participates in discriminant analysis on the
    /// same terms as an inline object type. `None` if any member has no shape.
    pub(super) fn union_member_shapes(
        &self,
        members: &[Type],
    ) -> Option<Vec<BTreeMap<String, ObjectField>>> {
        members.iter().map(|m| self.member_shape(m)).collect()
    }

    /// Whether every name this type *renders* would resolve, to that same
    /// declaration, in the module being typechecked. A union reached through an
    /// imported value carries member types the module never imported, and a
    /// diagnostic naming one in an `as` or `instanceof` produces `unknown type`
    /// — for a class receiver, on top of the very error the help was meant to
    /// resolve.
    ///
    /// The whole rendering has to be checked, not just the head: `Box<Alpha>`
    /// and `{ f: number; item: Alpha }` are both unusable in a module that
    /// imported `Box` but not `Alpha`.
    pub(super) fn name_resolves_here(&self, ty: &Type) -> bool {
        // `Display` is the one place that does not `peel`, and this function is
        // entirely about what `Display` writes: an alias renders its own name,
        // never its target's, so the alias name is what has to resolve. An
        // `AliasRef` is the recursion back-edge — descending it would not
        // terminate, and its own name is the whole rendering anyway.
        if let Type::Alias {
            mangled,
            name,
            args,
            ..
        }
        | Type::AliasRef {
            mangled,
            name,
            args,
            ..
        } = ty
        {
            return self.name_matches_here(name, mangled)
                && args.iter().all(|a| self.name_resolves_here(a));
        }
        if let Some((mangled, _, name, args)) = ty.interface_routing()
            // `interface_routing` sends every structural type to the prelude
            // `Object`, whose name is not what such a type renders — it renders
            // its shape, so only the shape's own names need checking.
            && matches!(ty.peel(), Type::InterfaceRef { .. } | Type::ClassRef { .. })
        {
            return self.name_matches_here(name, &mangled)
                && args.iter().all(|a| self.name_resolves_here(a));
        }
        match ty.peel() {
            Type::Object { fields, index } => {
                index
                    .as_ref()
                    .is_none_or(|i| self.name_resolves_here(&i.value))
                    && fields.values().all(|f| self.name_resolves_here(&f.ty))
            }
            Type::Array(elem) => self.name_resolves_here(elem),
            Type::Tuple(elements) | Type::Union(elements) => {
                elements.iter().all(|e| self.name_resolves_here(e))
            }
            Type::Function { params, ret, .. } => {
                params.iter().all(|p| self.name_resolves_here(p)) && self.name_resolves_here(ret)
            }
            Type::NumberEnum { mangled, name, .. } | Type::StringEnum { mangled, name, .. } => {
                self.name_matches_here(name, mangled)
            }
            _ => true,
        }
    }

    /// Whether `name` resolves here to the declaration `mangled` names. The
    /// mangled compare is what stops a same-named local type from vouching for
    /// an imported one.
    fn name_matches_here(&self, name: &str, mangled: &crate::MangledName) -> bool {
        self.lookup_named_type(name)
            .is_some_and(|sym| sym.mangled_name == *mangled)
    }

    /// What a write to `field` on one member targets, or `None` when the member
    /// has no such field at all.
    ///
    /// The `ty` is deliberately not the *read* type: an accessor pair may take
    /// wider than it returns — `get v(): number` beside `set v(s: number | null)`
    /// — so checking a value against the getter rejects assignments the language
    /// accepts.
    ///
    /// Dispatches as [`union_member_field_read_ty`](Self::union_member_field_read_ty)
    /// does, rather than through [`member_shape`](Self::member_shape): a class's
    /// structural form drops non-public members, so a `private` field would read
    /// as absent here while the read classifier — which resolves through
    /// `class_field_visible` and so honours module scope — sees it. The two
    /// disagreeing is what makes the write reporter claim a field is readonly
    /// when it is merely private.
    pub(super) fn union_member_field_write(
        &self,
        member: &Type,
        field: &str,
    ) -> Option<FieldWrite> {
        // A method is fixed at its declaration: no shape to write and no
        // permission to write it.
        if self.find_method(member, field).is_some() {
            return None;
        }
        match member.peel() {
            Type::Object { fields, index } => {
                let Some(f) = fields.get(field) else {
                    return index.as_ref().map(|i| FieldWrite {
                        ty: (*i.value).clone(),
                        writable: !i.readonly,
                    });
                };
                Some(FieldWrite {
                    ty: ObjectField::widen_optional(f.optional, f.ty.clone()),
                    writable: !f.readonly,
                })
            }
            Type::ClassRef { mangled, args, .. } => self.class_field_write(mangled, args, field),
            _ => self.property_field_write(member, field),
        }
    }

    /// [`union_member_field_write`](Self::union_member_field_write) for every
    /// receiver that reaches its members through a declared property.
    ///
    /// Not just an `InterfaceRef`: `string`, `T[]`, a tuple and `Uint8Array` all
    /// route here through `interface_routing`, and `infer_assign_field` reaches
    /// them the same way via `find_property`. Answering by variant instead leaves
    /// a nullable receiver disagreeing with its own non-null twin about what a
    /// value is checked against; the read side routes the same way, and says why
    /// in `access_after_null`.
    fn property_field_write(&self, member: &Type, field: &str) -> Option<FieldWrite> {
        let (sig, bindings, _, dispatch) = self.lookup_interface_property(member, field)?;
        let ty = if bindings.is_empty() {
            sig.ty.clone()
        } else {
            substitute_or_record(&sig.ty, &bindings, &self.type_limits)
        };
        Some(FieldWrite {
            ty: ObjectField::widen_optional(sig.optional, ty),
            // A Direct/Static property reads through a getter import keyed by its
            // own interface, which no union-receiver write can emit. Stricter than
            // `infer_assign_field`, which accepts one through `find_property` on a
            // single receiver — latent today, since every writable host property is
            // on a VTable interface.
            writable: !sig.readonly && dispatch == crate::Dispatch::VTable,
        })
    }

    /// [`union_member_field_write`](Self::union_member_field_write) for a class
    /// receiver, where three sources answer in a fixed order.
    ///
    /// An accessor synthesizes a `fields` entry, so both accessor checks have to
    /// come before `class_field_visible` or a property would answer with its
    /// *read* type. A getter with no setter takes nothing at all, and
    /// `infer_assign_field` checks such a write against `Type::Error`; the two
    /// must not disagree, or the hint describes a type the value is never held to.
    fn class_field_write(
        &self,
        mangled: &crate::MangledName,
        args: &[Type],
        field: &str,
    ) -> Option<FieldWrite> {
        if let Some(setter_ty) = self.class_setter(mangled, args, field) {
            return Some(FieldWrite {
                ty: setter_ty,
                writable: true,
            });
        }
        if self.class_getter(mangled, args, field).is_some() {
            return Some(FieldWrite {
                ty: Type::Error,
                writable: false,
            });
        }
        let (sig, _) = self.class_field_visible(mangled, args, field)?;
        Some(FieldWrite {
            ty: ObjectField::widen_optional(sig.optional, sig.ty.clone()),
            writable: !sig.readonly,
        })
    }

    /// Whether `field` can be written through this union member at all.
    pub(super) fn union_member_field_writable(&self, member: &Type, field: &str) -> bool {
        self.union_member_field_write(member, field)
            .is_some_and(|w| w.writable)
    }

    /// [`narrowing::union_discriminant`](super::narrowing::union_discriminant)
    /// with nominal members expanded through the type registry. The free
    /// function sees inline object shapes only; prefer this wherever an
    /// `Inferer` is in hand.
    pub(super) fn union_discriminant_with_nominals(
        &self,
        members: &[Type],
    ) -> Option<super::narrowing::Discriminant> {
        let shapes = self.union_member_shapes(members)?;
        super::narrowing::discriminant_from_shapes(&shapes)
    }

    /// Whether a type carries readable fields at all — the shapes
    /// [`Self::union_member_field_read_ty`] resolves, whatever their names. A
    /// `null` member fails here, which is what keeps `T | null` reporting "may
    /// be null" rather than "field missing on `null`".
    pub(super) fn is_field_bearing(member: &Type) -> bool {
        matches!(
            member.peel(),
            Type::Object { .. } | Type::InterfaceRef { .. } | Type::ClassRef { .. }
        )
    }

    pub(super) fn find_property(
        &self,
        recv_ty: &Type,
        name: &str,
    ) -> Option<(
        PropertySig,
        BTreeMap<String, Type>,
        crate::MangledName,
        crate::Dispatch,
    )> {
        let found = self.lookup_interface_property(recv_ty, name)?;
        // VTable-dispatched interfaces have no prelude-exported property
        // getters; their reads route through `FieldAccess` shape dispatch in
        // `infer_field_access`. Callers of `find_property` (property writes,
        // postfix, optional-chain field lookup) only support the getter path.
        if found.3 == crate::Dispatch::VTable {
            return None;
        }
        Some(found)
    }

    /// Field map for an assignment / postfix target: an inline object type, or a
    /// named interface resolved to its structural property shape. `None` for any
    /// other receiver (callers emit "cannot assign to field of `T`"). This is
    /// what lets `iface.field = x` work the same as `{ field: T }`'s `.field = x`.
    pub(super) fn assignment_target_fields(
        &self,
        recv_ty: &Type,
    ) -> Option<BTreeMap<String, ObjectField>> {
        match recv_ty.peel() {
            Type::Object { fields, .. } => Some(fields.clone()),
            Type::InterfaceRef {
                mangled,
                name,
                args,
                ..
            } => self.structural_form(mangled, name, args),
            _ => None,
        }
    }

    /// The structural member map of a named type — an interface's methods and
    /// properties, or a class's public member surface. Backs member access,
    /// `for-of` dispatch, and structural unification.
    pub(super) fn structural_form(
        &self,
        mangled: &crate::MangledName,
        name: &str,
        args: &[Type],
    ) -> Option<BTreeMap<String, ObjectField>> {
        let sym = self.lookup_structural_type(mangled, name)?;
        // A class's structural form (a class instance passed where an
        // Iterable/shape is expected): the substituted public member map, same
        // shape as the interface path below.
        if matches!(&sym.kind, TypeKind::Class { .. }) {
            return self.resolver().class_full_form(mangled, args);
        }
        let TypeKind::Interface {
            generics,
            methods,
            properties,
            ..
        } = &sym.kind
        else {
            return None;
        };
        let bindings: BTreeMap<String, Type> =
            generics.iter().cloned().zip(args.iter().cloned()).collect();
        let mut result: BTreeMap<String, ObjectField> = BTreeMap::new();
        for (name, sig) in methods {
            // Method-level generics can't be represented as object-literal field shapes.
            if !sig.generics.is_empty() {
                continue;
            }
            let params: Vec<Type> = sig
                .params
                .iter()
                .map(|p| substitute_or_record(&p.ty, &bindings, &self.type_limits))
                .collect();
            let ret = substitute_or_record(&sig.ret, &bindings, &self.type_limits);
            result.insert(
                name.clone(),
                ObjectField {
                    ty: Type::Function {
                        params,
                        ret: Box::new(ret),
                        predicate: None,
                        has_rest: false,
                    },
                    optional: false,
                    readonly: true,
                },
            );
        }
        for (name, sig) in properties {
            let ty = substitute_or_record(&sig.ty, &bindings, &self.type_limits);
            result.insert(
                name.clone(),
                ObjectField {
                    ty,
                    optional: sig.optional,
                    readonly: sig.readonly,
                },
            );
        }
        Some(result)
    }

    /// Like [`structural_form`](Self::structural_form) but **properties
    /// only** — methods are omitted. Used by the `as` cast's runtime structural check:
    /// interface methods aren't stored as object fields at runtime, so a plain data object
    /// can't be checked for their presence; only data properties are verifiable. Thin
    /// wrapper over [`TypeResolver::interface_data_shape`](super::assignable::TypeResolver),
    /// the shared object↔interface expansion.
    pub(super) fn interface_data_shape(
        &self,
        iface_mangled: &crate::MangledName,
        iface_name: &str,
        iface_args: &[Type],
    ) -> Option<BTreeMap<String, ObjectField>> {
        self.resolver()
            .interface_data_shape(iface_mangled, iface_name, iface_args)
    }
}

/// `sig` as a union of arrays offers it through its joined element type, which
/// is sound only where elements flow out: `None` for a method taking an element,
/// which would have to suit every member at once (tsc intersects the members'
/// parameters). A callback's array argument is the receiver itself, so it
/// becomes `readonly`.
fn restrict_to_reads(mut sig: MethodSig, generics: &[String]) -> Option<MethodSig> {
    if sig
        .params
        .iter()
        .any(|param| generic_flows_in(&param.ty, generics))
    {
        return None;
    }
    for param in &mut sig.params {
        param.ty = readonly_callback_arrays(&param.ty, generics);
    }
    Some(sig)
}

/// `ty` with each array of an interface generic that a function parameter takes
/// made `readonly`, at any depth of callback.
fn readonly_callback_arrays(ty: &Type, generics: &[String]) -> Type {
    match ty {
        Type::Function {
            params,
            ret,
            predicate,
            has_rest,
        } => Type::Function {
            params: params
                .iter()
                .map(|param| match param {
                    Type::Array(element)
                        if matches!(element.as_ref(), Type::TypeVar(name) if generics.contains(name)) =>
                    {
                        Type::Readonly(Box::new(param.clone()))
                    }
                    _ => readonly_callback_arrays(param, generics),
                })
                .collect(),
            ret: ret.clone(),
            predicate: predicate.clone(),
            has_rest: *has_rest,
        },
        Type::Union(members) => Type::Union(
            members
                .iter()
                .map(|member| readonly_callback_arrays(member, generics))
                .collect(),
        ),
        _ => ty.clone(),
    }
}

/// Whether one of `generics` appears in `ty` other than as a parameter of a
/// function type, where a callback receives it and so reads it.
fn generic_flows_in(ty: &Type, generics: &[String]) -> bool {
    match ty {
        Type::TypeVar(name) => generics.contains(name),
        Type::Function { ret, .. } => generic_flows_in(ret, generics),
        Type::Array(element) | Type::Readonly(element) => generic_flows_in(element, generics),
        Type::Tuple(elements) | Type::Union(elements) => elements
            .iter()
            .any(|element| generic_flows_in(element, generics)),
        Type::Object { fields, index } => {
            index
                .as_ref()
                .is_some_and(|index| generic_flows_in(&index.value, generics))
                || fields
                    .values()
                    .any(|field| generic_flows_in(&field.ty, generics))
        }
        Type::InterfaceRef { args, .. }
        | Type::ClassRef { args, .. }
        | Type::AliasRef { args, .. } => args.iter().any(|arg| generic_flows_in(arg, generics)),
        Type::Alias { args, ty, .. } => {
            args.iter().any(|arg| generic_flows_in(arg, generics)) || generic_flows_in(ty, generics)
        }
        _ => false,
    }
}
