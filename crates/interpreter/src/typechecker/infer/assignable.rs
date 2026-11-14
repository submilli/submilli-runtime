//! Type-level assignability rule + small literal-bridging helpers used
//! by the predicate-environment extractor.

use std::collections::BTreeMap;
use std::ops::ControlFlow;

use crate::{MangledName, MethodSig, ObjectField, Type, TypeKind};

use super::generic::substitute_or_record;
use super::narrowing;
use super::type_namespace::TypeNamespace;
use super::type_registry::TypeRegistry;
use super::variance::Variance;

/// The pair of tables structural resolution needs: the import-scoped namespace
/// plus the import-independent FQN registry. Carried (by `Copy`) wherever
/// assignability recurses so a recursion back-edge into a *library* alias still
/// resolves by its own package, not just what the current module imported.
#[derive(Clone, Copy)]
pub(crate) struct TypeResolver<'a> {
    pub(in crate::typechecker) types: &'a TypeNamespace<'a>,
    pub(in crate::typechecker) registry: &'a TypeRegistry<'a>,
    /// Records an oversized type met while resolving, for the inferer's next
    /// checkpoint; the resolver itself answers as if the type were `Error`.
    pub(crate) limits: &'a crate::type_size::TypeLimits,
    /// Relate by tsc's strict subtype relation instead of assignability; see
    /// [`strict_subtype`].
    pub(crate) strict_subtype: bool,
    pub(in crate::typechecker) generic_constraints: &'a BTreeMap<u32, Type>,
}

/// A single way a class fails to satisfy an interface it declares it `implements`.
pub(super) enum ImplementsFailure {
    /// The interface requires `member`, but the class has no public member by
    /// that name (private members don't count toward the contract).
    Missing(String),
    /// `member` exists on the class but its type is not assignable to the
    /// interface's — wrong type or wrong arity. Signatures are pre-rendered
    /// (`Display`) for the diagnostic.
    Incompatible {
        member: String,
        expected: String,
        actual: String,
    },
    /// `member` has the right type but cannot be written (`readonly` field, or a
    /// getter with no setter) while the interface declares it settable. Reported
    /// apart from [`Incompatible`] because both sides render the same type there,
    /// naming no fix.
    NotWritable {
        member: String,
        ty: String,
    },
    NotReadable {
        member: String,
        ty: String,
    },
    /// `member` is optional on the class where the interface requires it. Same
    /// identical-types problem as [`NotWritable`].
    OptionalityMismatch {
        member: String,
        ty: String,
    },
    /// The interface declares a call or construct signature (the rendered
    /// signature), which no class instance has.
    Signature(String),
}

impl<'a> TypeResolver<'a> {
    pub(super) fn lookup(
        &self,
        mangled: &MangledName,
        name: &str,
    ) -> Option<&'a crate::TypeSymbol> {
        self.registry
            .lookup(mangled)
            .or_else(|| self.types.lookup(name))
    }

    /// Full structural member form of an interface — methods (as `Function`
    /// types, method-level generics skipped) plus data properties, generic
    /// args substituted. Backs structural interface assignability.
    pub(super) fn interface_full_form(
        &self,
        mangled: &MangledName,
        name: &str,
        args: &[Type],
    ) -> Option<BTreeMap<String, ObjectField>> {
        let TypeKind::Interface {
            generics,
            methods,
            properties,
            ..
        } = &self.lookup(mangled, name)?.kind
        else {
            return None;
        };
        let bindings: BTreeMap<String, Type> =
            generics.iter().cloned().zip(args.iter().cloned()).collect();
        let mut out: BTreeMap<String, ObjectField> = BTreeMap::new();
        for (member, sig) in methods {
            if sig
                .signatures()
                .any(|signature| !signature.generics.is_empty())
            {
                continue;
            }
            out.insert(
                member.clone(),
                ObjectField {
                    ty: self.method_type(sig, &bindings),
                    optional: sig.optional,
                    readonly: true,
                    method: true,
                },
            );
        }
        for (member, sig) in properties {
            let ty = if methods.contains_key(member) {
                self.callable_value_constraint(&sig.ty)
            } else {
                sig.ty.clone()
            };
            if methods.contains_key(member) && ty == Type::Unknown {
                continue;
            }
            out.insert(
                member.clone(),
                ObjectField {
                    ty: substitute_or_record(&ty, &bindings, self.limits),
                    optional: sig.optional
                        && methods.get(member).is_none_or(|method| method.optional),
                    readonly: sig.readonly,
                    method: false,
                },
            );
        }
        Some(out)
    }

    /// An interface's call or construct signature as a function type, its
    /// generic arguments substituted. A signature that declares type
    /// parameters keeps them as type variables.
    pub(super) fn interface_signature(
        &self,
        mangled: &MangledName,
        name: &str,
        args: &[Type],
        which: Signature,
    ) -> Option<Type> {
        let TypeKind::Interface {
            generics,
            call,
            construct,
            ..
        } = &self.lookup(mangled, name)?.kind
        else {
            return None;
        };
        let sig = match which {
            Signature::Call => call.as_ref()?,
            Signature::Construct => construct.as_ref()?,
        };
        let bindings: BTreeMap<String, Type> =
            generics.iter().cloned().zip(args.iter().cloned()).collect();
        Some(self.method_type(sig, &bindings))
    }

    /// `interface_signature` of the interface `ty` names.
    pub(super) fn signature_of(&self, ty: &Type, which: Signature) -> Option<Type> {
        let (mangled, _, name, args) = ty.interface_routing()?;
        self.interface_signature(&mangled, name, &args, which)
    }

    pub(super) fn sym_by_mangled(&self, mangled: &MangledName) -> Option<&'a crate::TypeSymbol> {
        self.registry
            .lookup(mangled)
            .or_else(|| self.types.lookup_by_mangled(mangled))
    }

    /// True when `actual` is `expected` or a subclass of it (nominal, via the
    /// `extends` chain). Cycle-guarded.
    pub(super) fn is_subclass(&self, actual: &MangledName, expected: &MangledName) -> bool {
        let mut cur = Some(actual.clone());
        let mut seen: Vec<MangledName> = Vec::new();
        while let Some(m) = cur {
            if &m == expected {
                return true;
            }
            if seen.contains(&m) {
                break;
            }
            seen.push(m.clone());
            cur = match self.sym_by_mangled(&m).map(|s| &s.kind) {
                Some(TypeKind::Class { extends, .. }) => extends.as_ref().map(|e| e.parent.clone()),
                _ => None,
            };
        }
        false
    }

    /// The type args at which `actual` (instantiated at `actual_args`) reaches
    /// ancestor `expected` through its `extends` chain — each hop substitutes
    /// the clause's args with the child's bindings. `None` when `expected`
    /// isn't an ancestor. Powers subclass→generic-parent assignability
    /// (`NumBox` extends `Box<number>` reaches `Box` at `[number]`, so
    /// `NumBox` is not a `Box<string>`).
    pub(super) fn class_args_at_ancestor(
        &self,
        actual: &MangledName,
        actual_args: &[Type],
        expected: &MangledName,
    ) -> Option<Vec<Type>> {
        super::classes::walk_class_chain_with(
            |m| self.sym_by_mangled(m).cloned(),
            self.limits,
            actual,
            actual_args,
            |sym, bindings| {
                if &sym.mangled_name != expected {
                    return ControlFlow::Continue(());
                }
                // The bindings at `expected` are its own generics zipped with
                // the args this chain reached it at.
                let TypeKind::Class { generics, .. } = &sym.kind else {
                    return ControlFlow::Break(None);
                };
                ControlFlow::Break(Some(
                    generics
                        .iter()
                        .map(|g| bindings.get(g).cloned().unwrap_or(Type::Unknown))
                        .collect(),
                ))
            },
        )
    }

    /// Public structural member form of a class — public methods (as `Function`
    /// types) and public fields, including inherited members (child wins on
    /// name). Backs `implements` conformance and class→interface assignability.
    /// Private members are excluded: they are not part of the external contract.
    /// `args` substitutes the class's own generics; leaving them raw would make
    /// members leak `TypeVar`s, which `assignable_rec` treats as wildcards.
    pub(super) fn class_full_form(
        &self,
        mangled: &MangledName,
        args: &[Type],
    ) -> Option<BTreeMap<String, ObjectField>> {
        let mut out: BTreeMap<String, ObjectField> = BTreeMap::new();
        super::classes::for_each_class_in_chain(
            |m| self.sym_by_mangled(m).cloned(),
            self.limits,
            mangled,
            args,
            |sym, bindings| {
                let TypeKind::Class {
                    fields,
                    methods,
                    method_visibility,
                    ..
                } = &sym.kind
                else {
                    return;
                };
                for (name, sig) in methods {
                    if sig
                        .signatures()
                        .any(|signature| !signature.generics.is_empty())
                        || method_visibility.get(name).copied() == Some(crate::Visibility::Private)
                    {
                        continue;
                    }
                    out.entry(name.clone()).or_insert(ObjectField {
                        ty: self.method_type(sig, bindings),
                        optional: sig.optional,
                        readonly: true,
                        method: true,
                    });
                }
                for (name, f) in fields {
                    if f.visibility == crate::Visibility::Private {
                        continue;
                    }
                    out.entry(name.clone()).or_insert(ObjectField {
                        ty: substitute_or_record(&f.ty, bindings, self.limits),
                        optional: f.optional,
                        readonly: f.readonly,
                        method: false,
                    });
                }
            },
        )?;
        Some(out)
    }

    /// A method's signature as a function type, with the class's arguments
    /// substituted.
    pub(super) fn method_type(&self, sig: &MethodSig, bindings: &BTreeMap<String, Type>) -> Type {
        if !sig.overloads.is_empty() {
            return Type::intersection(
                sig.signatures()
                    .map(|signature| self.method_type(&signature, bindings))
                    .collect(),
            );
        }
        Type::Function {
            params: sig
                .params
                .iter()
                .map(|p| {
                    crate::ObjectField::widen_optional(
                        p.is_omittable(),
                        substitute_or_record(&p.ty, bindings, self.limits),
                    )
                })
                .collect(),
            ret: Box::new(substitute_or_record(&sig.ret, bindings, self.limits)),
            predicate: None,
            has_rest: sig.params.last().is_some_and(|p| p.rest),
            optional: crate::package_declaration::optional_parameter_count(&sig.params),
        }
    }

    /// The private fields and methods of the class and its ancestors, the
    /// members [`class_full_form`](Self::class_full_form) leaves out, with the
    /// class's arguments substituted. Each is keyed by the class that declares
    /// it and its name. `None` means the `extends` chain broke.
    pub(super) fn class_private_members(
        &self,
        mangled: &MangledName,
        args: &[Type],
    ) -> Option<PrivateMembers> {
        let mut private = BTreeMap::new();
        super::classes::for_each_class_in_chain(
            |m| self.sym_by_mangled(m).cloned(),
            self.limits,
            mangled,
            args,
            |sym, bindings| {
                let TypeKind::Class {
                    fields,
                    methods,
                    method_visibility,
                    ..
                } = &sym.kind
                else {
                    return;
                };
                let mut insert = |name: &String, ty: Type| {
                    private.insert((sym.mangled_name.clone(), name.clone()), ty);
                };
                for (name, field) in fields {
                    if field.visibility == crate::Visibility::Private {
                        insert(name, substitute_or_record(&field.ty, bindings, self.limits));
                    }
                }
                for (name, sig) in methods {
                    if method_visibility.get(name).copied() == Some(crate::Visibility::Private) {
                        insert(name, self.method_type(sig, bindings));
                    }
                }
            },
        )?;
        Some(private)
    }

    /// Per-member conformance of a class against one interface it declares it
    /// `implements`. An empty result means the class satisfies the interface.
    /// This mirrors the class→interface arm of [`assignable_rec`] but reports
    /// *which* members fail (powering member-level diagnostics) instead of
    /// collapsing to a single boolean.
    /// `class_args` instantiates a generic class's own parameters. They must be
    /// opaque (`GenericParam`), not bare `TypeVar`s: `assignable_rec` treats a
    /// bare `TypeVar` as a wildcard, which would make every member of every
    /// generic class satisfy every interface silently.
    pub(super) fn implements_failures(
        &self,
        class_mangled: &MangledName,
        class_args: &[Type],
        iface_mangled: &MangledName,
        iface_name: &str,
        iface_args: &[Type],
    ) -> Vec<ImplementsFailure> {
        let Some(iface_form) = self.interface_full_form(iface_mangled, iface_name, iface_args)
        else {
            return Vec::new();
        };
        let mut failures = self.implements_form_failures(class_mangled, class_args, &iface_form);
        for (which, prefix) in [(Signature::Call, ""), (Signature::Construct, "new ")] {
            if let Some(signature) =
                self.interface_signature(iface_mangled, iface_name, iface_args, which)
            {
                failures.push(ImplementsFailure::Signature(format!("{prefix}{signature}")));
            }
        }
        let actual = Type::class_ref(
            crate::Package("".into()),
            "",
            class_mangled.clone(),
            class_args.to_vec(),
        );
        let expected = Type::interface_ref(
            crate::Package("".into()),
            iface_name,
            iface_mangled.clone(),
            iface_args.to_vec(),
        );
        failures.retain(|failure| match failure {
            ImplementsFailure::Missing(member) | ImplementsFailure::Incompatible { member, .. } => {
                !iface_form.get(member).is_none_or(|field| field.method)
                    || self.method_relation(&actual, &expected, member, &mut Vec::new())
                        != Some(true)
            }
            _ => true,
        });
        if let Some(symbol) = self.lookup(iface_mangled, iface_name)
            && let TypeKind::Interface { methods, .. } = &symbol.kind
        {
            for (member, signature) in methods {
                if signature.generics.is_empty() && signature.overloads.is_empty() {
                    continue;
                }
                let compatible = self.method_relation(&actual, &expected, member, &mut Vec::new());
                if compatible != Some(true)
                    && !(compatible.is_none() && signature.optional)
                    && !failures.iter().any(|failure| match failure {
                        ImplementsFailure::Missing(name)
                        | ImplementsFailure::Incompatible { member: name, .. }
                        | ImplementsFailure::NotReadable { member: name, .. }
                        | ImplementsFailure::NotWritable { member: name, .. }
                        | ImplementsFailure::OptionalityMismatch { member: name, .. } => {
                            name == member
                        }
                        ImplementsFailure::Signature(_) => false,
                    })
                {
                    failures.push(ImplementsFailure::Incompatible {
                        member: member.clone(),
                        expected: self.method_type(signature, &BTreeMap::new()).to_string(),
                        actual: "method signature".into(),
                    });
                }
            }
        }
        failures
    }

    /// The members of `class_mangled` at `class_args` that fail an `implements`
    /// target's members, `iface_form`: an interface's or an object type's.
    pub(super) fn implements_form_failures(
        &self,
        class_mangled: &MangledName,
        class_args: &[Type],
        iface_form: &BTreeMap<String, ObjectField>,
    ) -> Vec<ImplementsFailure> {
        let Some(class_form) = self.class_full_form(class_mangled, class_args) else {
            return Vec::new();
        };
        let mut failures = Vec::new();
        for (member, exp) in iface_form {
            match class_form.get(member) {
                Some(act) => {
                    let mut seen = Vec::new();
                    // A setter's parameter is not a readable value. Check for a
                    // getter and required presence before comparing read types:
                    // optional reads add undefined even when declared types match.
                    //
                    // `readonly` is shallow, so the type check stays covariant. But a
                    // writable interface member requires writable class storage,
                    // excluding readonly fields and getter-only accessors. Real
                    // methods have callable payloads that a structural interface
                    // can replace, even though their structural form is readonly.
                    if self.class_property_is_write_only(class_mangled, class_args, member) {
                        failures.push(ImplementsFailure::NotReadable {
                            member: member.clone(),
                            ty: exp.ty.to_string(),
                        });
                    } else if !exp.optional && act.optional {
                        failures.push(ImplementsFailure::OptionalityMismatch {
                            member: member.clone(),
                            ty: exp.ty.to_string(),
                        });
                    } else if !self.class_member_assignable(
                        class_mangled,
                        class_args,
                        member,
                        &act.read_ty(),
                        &exp.read_ty(),
                        &mut seen,
                    ) {
                        failures.push(ImplementsFailure::Incompatible {
                            member: member.clone(),
                            expected: exp.ty.to_string(),
                            actual: act.ty.to_string(),
                        });
                    } else if !exp.readonly
                        && act.readonly
                        && !self.class_member_is_method(class_mangled, class_args, member)
                    {
                        failures.push(ImplementsFailure::NotWritable {
                            member: member.clone(),
                            ty: exp.ty.to_string(),
                        });
                    }
                }
                None if !exp.optional => failures.push(ImplementsFailure::Missing(member.clone())),
                None => {}
            }
        }
        failures
    }

    /// Structural calls resolve a class method dynamically, retaining its default
    /// metadata even when the target signature has fewer parameters.
    fn class_member_assignable(
        self,
        class: &MangledName,
        args: &[Type],
        member: &str,
        actual: &Type,
        expected: &Type,
        seen: &mut Vec<(Type, Type)>,
    ) -> bool {
        if assignable_rec(actual, expected, self, seen) {
            return true;
        }
        if let Type::Union(members) = expected.peel() {
            return members.iter().any(|expected| {
                self.class_member_assignable(class, args, member, actual, expected, seen)
            });
        }
        let (
            Type::Function {
                params,
                ret,
                predicate,
                optional: _,
                has_rest: false,
            },
            Type::Function {
                params: expected_params,
                has_rest: false,
                ..
            },
        ) = (actual.peel(), expected.peel())
        else {
            return false;
        };
        if expected_params.len() >= params.len() {
            return false;
        }
        let omittable = super::classes::walk_class_chain_with(
            |m| self.sym_by_mangled(m).cloned(),
            self.limits,
            class,
            args,
            |sym, _| {
                let TypeKind::Class {
                    methods, fields, ..
                } = &sym.kind
                else {
                    return ControlFlow::Continue(());
                };
                if fields.contains_key(member) {
                    return ControlFlow::Break(Some(false));
                }
                let Some(method) = methods.get(member) else {
                    return ControlFlow::Continue(());
                };
                ControlFlow::Break(Some(
                    method
                        .params
                        .iter()
                        .skip(expected_params.len())
                        .all(|param| param.default.is_some()),
                ))
            },
        )
        .unwrap_or(false);
        omittable
            && assignable_rec(
                &Type::Function {
                    params: params.iter().take(expected_params.len()).cloned().collect(),
                    ret: ret.clone(),
                    predicate: predicate.clone(),
                    has_rest: false,
                    optional: 0,
                },
                expected,
                self,
                seen,
            )
    }

    fn class_member_is_method(&self, mangled: &MangledName, args: &[Type], member: &str) -> bool {
        super::classes::walk_class_chain_with(
            |name| self.sym_by_mangled(name).cloned(),
            self.limits,
            mangled,
            args,
            |symbol, _| {
                let TypeKind::Class {
                    methods, fields, ..
                } = &symbol.kind
                else {
                    return ControlFlow::Continue(());
                };
                if fields.contains_key(member) {
                    return ControlFlow::Break(Some(false));
                }
                if methods.contains_key(member) {
                    return ControlFlow::Break(Some(true));
                }
                ControlFlow::Continue(())
            },
        )
        .unwrap_or(false)
    }

    fn class_property_is_write_only(
        &self,
        mangled: &MangledName,
        args: &[Type],
        member: &str,
    ) -> bool {
        let mut has_setter = false;
        let mut has_getter = false;
        super::classes::for_each_class_in_chain(
            |m| self.sym_by_mangled(m).cloned(),
            self.limits,
            mangled,
            args,
            |sym, _| {
                if let TypeKind::Class { accessors, .. } = &sym.kind {
                    for accessor in accessors.iter().filter(|a| a.name() == member) {
                        match accessor {
                            crate::AccessorSig::Getter { .. } => has_getter = true,
                            crate::AccessorSig::Setter { .. } => has_setter = true,
                        }
                    }
                }
            },
        );
        has_setter && !has_getter
    }

    /// Structural object shape of a **data-only** interface's properties, with
    /// generic args substituted. `None` if the name doesn't resolve to an
    /// interface, or if the interface declares methods — a method-bearing
    /// interface (`Map`, `Iterator`, …) is nominal: a plain object has no
    /// vtable, so neither structural assignability nor a runtime structural
    /// check can treat it as data. The single source of truth for
    /// object↔interface expansion shared by the assignability arms, the
    /// `as`-cast reducer
    /// ([`Inferer::interface_data_shape`](super::Inferer::interface_data_shape)
    /// delegates here), and JSON.parse target validation.
    pub(super) fn interface_data_shape(
        &self,
        mangled: &MangledName,
        name: &str,
        args: &[Type],
    ) -> Option<BTreeMap<String, ObjectField>> {
        let kind = &self.lookup(mangled, name)?.kind;
        let TypeKind::Interface {
            generics,
            properties,
            methods,
            ..
        } = kind
        else {
            return None;
        };
        if !methods.is_empty() || kind.has_signature() {
            return None;
        }
        let bindings: BTreeMap<String, Type> =
            generics.iter().cloned().zip(args.iter().cloned()).collect();
        Some(
            properties
                .iter()
                .map(|(field, sig)| {
                    (
                        field.clone(),
                        ObjectField {
                            ty: substitute_or_record(&sig.ty, &bindings, self.limits),
                            optional: sig.optional,
                            readonly: sig.readonly,
                            method: false,
                        },
                    )
                })
                .collect(),
        )
    }

    /// The names of an interface's methods.
    pub(super) fn interface_method_names(&self, mangled: &MangledName, name: &str) -> Vec<String> {
        match self.lookup(mangled, name).map(|symbol| &symbol.kind) {
            Some(TypeKind::Interface { methods, .. }) => methods.keys().cloned().collect(),
            _ => Vec::new(),
        }
    }

    /// The names of a class's methods, its ancestors' included.
    pub(super) fn class_method_names(&self, mangled: &MangledName, args: &[Type]) -> Vec<String> {
        let mut names = Vec::new();
        super::classes::for_each_class_in_chain(
            |m| self.sym_by_mangled(m).cloned(),
            self.limits,
            mangled,
            args,
            |sym, _| {
                if let TypeKind::Class { methods, .. } = &sym.kind {
                    names.extend(methods.keys().cloned());
                }
            },
        );
        names
    }

    /// True when `name` resolves to an interface that declares methods or
    /// signatures — the nominal-only interfaces `interface_data_shape` refuses
    /// to expand.
    pub(super) fn interface_has_methods(&self, mangled: &MangledName, name: &str) -> bool {
        match self.lookup(mangled, name).map(|symbol| &symbol.kind) {
            Some(kind @ TypeKind::Interface { methods, .. }) => {
                !methods.is_empty() || kind.has_signature()
            }
            _ => false,
        }
    }
}

/// Whether a function with a rest parameter, `actual`, accepts every argument
/// list of a fixed-arity function, `expected`, as in tsc: each argument at a
/// fixed position fits that parameter (`accepts(expected, actual)`), and each
/// past them fits the rest parameter's element type.
///
/// Unlike tsc, the two must differ in parameter count: a rest function's
/// closure has the same Wasm arity as a fixed function with as many
/// parameters, so a cast from an erased slot could not tell it needs its
/// arguments packed.
pub(crate) fn rest_function_accepts(
    actual: &[Type],
    expected: &[Type],
    mut accepts: impl FnMut(&Type, &Type) -> bool,
) -> bool {
    let Some((rest, fixed)) = actual.split_last() else {
        return false;
    };
    let Type::Array(element) = rest.peel() else {
        return false;
    };
    let Some(past_fixed) = expected.get(fixed.len()..) else {
        return false;
    };
    actual.len() != expected.len()
        && fixed
            .iter()
            .zip(expected)
            .all(|(declared, passed)| accepts(passed, declared))
        && past_fixed.iter().all(|passed| accepts(passed, element))
}

/// Whether a value of an enum, or of its member `actual`, is one of `expected`:
/// the whole enum, or that same member.
fn enum_member_fits<V>(
    actual: Option<&crate::types::EnumMember<V>>,
    expected: Option<&crate::types::EnumMember<V>>,
) -> bool {
    match (actual, expected) {
        (_, None) => true,
        (Some(actual), Some(expected)) => actual.name == expected.name,
        (None, Some(_)) => false,
    }
}

/// The members of the enum `actual` as member literal types, when the union
/// `expected` names one of them: only then can it need them all.
fn enum_as_member_union(
    actual: &Type,
    expected: &[Type],
    types: TypeResolver,
) -> Option<Vec<Type>> {
    let (Type::NumberEnum {
        mangled: enum_mangled,
        member: None,
        ..
    }
    | Type::StringEnum {
        mangled: enum_mangled,
        member: None,
        ..
    }) = actual.peel()
    else {
        return None;
    };
    let names_member = expected.iter().any(|m| match m.peel() {
        Type::NumberEnum {
            mangled,
            member: Some(_),
            ..
        }
        | Type::StringEnum {
            mangled,
            member: Some(_),
            ..
        } => mangled == enum_mangled,
        _ => false,
    });
    if !names_member {
        return None;
    }
    super::comparable::enum_member_types(actual.peel(), types)
}

pub(crate) fn assignable(actual: &Type, expected: &Type, types: TypeResolver) -> bool {
    // Coinductive assumption set for recursive-alias (`AliasRef`)
    // expansion: a pair re-encountered mid-proof is assumed to hold, so
    // comparing recursive types terminates.
    let mut seen: Vec<(Type, Type)> = Vec::new();
    assignable_rec(actual, expected, types, &mut seen)
}

/// Whether `actual` is a subtype of `expected` by tsc's strict subtype
/// relation, which reduces a union where tsc does (`c ? a : b`, the results of
/// a union call): assignability, except that a primitive is no subtype of an
/// object type, an object type is no subtype of one with an optional field it
/// lacks, and a function is no subtype of one that takes fewer parameters or
/// requires an argument it lets a caller omit.
pub(crate) fn strict_subtype(actual: &Type, expected: &Type, types: TypeResolver) -> bool {
    let types = TypeResolver {
        strict_subtype: true,
        ..types
    };
    assignable(actual, expected, types)
}

/// The union of `types` with each member that is a strict subtype of another
/// left out, as tsc reduces a union call's results. Of members that are each
/// other's subtypes, the first stays.
pub(crate) fn strict_subtype_union(types: Vec<Type>, resolver: TypeResolver) -> Type {
    let mut members = match Type::union(types) {
        Type::Union(members) => members,
        single => return single,
    };
    let mut index = members.len();
    while index > 0 {
        index -= 1;
        let absorbed = members.iter().enumerate().any(|(other, member)| {
            other != index && strict_subtype(&members[index], member, resolver)
        });
        if absorbed {
            members.remove(index);
        }
    }
    Type::union(members)
}

pub(super) fn assignable_rec(
    actual: &Type,
    expected: &Type,
    types: TypeResolver,
    seen: &mut Vec<(Type, Type)>,
) -> bool {
    // Structures that each fit the type limits can still take exponentially
    // many comparisons, so each one draws on the phase's work allowance. Once a
    // limit is recorded the answer no longer matters: compilation fails.
    if !types.limits.spend_work(1) {
        return false;
    }
    if let Some(actual) = super::utility_types::evaluate_concrete_operator(actual, types) {
        return assignable_rec(&actual, expected, types, seen);
    }
    if let Some(expected) = super::utility_types::evaluate_concrete_operator(expected, types) {
        return assignable_rec(actual, &expected, types, seen);
    }
    if let Type::Intersection(members) = expected.without_aliases() {
        return members
            .iter()
            .all(|member| assignable_rec(actual, member, types, seen));
    }
    if let Type::Intersection(members) = actual.without_aliases() {
        // A union takes the intersection when one of its alternatives does,
        // the intersection itself included.
        if let Type::Union(alternatives) = expected.peel()
            && alternatives
                .iter()
                .any(|alternative| assignable_rec(actual, alternative, types, seen))
        {
            return true;
        }
        return members
            .iter()
            .any(|member| assignable_rec(member, expected, types, seen))
            || hybrid_satisfies_interface(actual, expected, types, seen);
    }
    // `NoInfer<X>` is `X` and `NonNullable<X>` is `X & {}`, before the
    // refinement rules below compare `X & {}` piecewise.
    if let Some(expected) = rewrite_to_operand_form(expected) {
        return assignable_rec(actual, &expected, types, seen);
    }
    if let Some(actual) = rewrite_to_operand_form(actual) {
        return assignable_rec(&actual, expected, types, seen);
    }
    // `unknown` and a type parameter fit a union that holds every value, as
    // in TypeScript.
    if matches!(actual.peel(), Type::Unknown | Type::GenericParam { .. })
        && expected.holds_every_value()
    {
        return true;
    }
    if matches!(actual.without_aliases(), Type::Refined { .. })
        && refinement_fits_union(actual, expected, types, seen)
    {
        return true;
    }
    if let Type::Refined { original, ty } = expected.without_aliases() {
        return assignable_rec(actual, original, types, seen)
            && assignable_rec(actual, ty, types, seen);
    }
    if let Type::Refined { original, ty } = actual.without_aliases() {
        // `Partial<T>[K]` without `undefined` is `T[K]`: `patch[k] ?? base[k]`.
        if let Some(MappedProperty::Optional(property)) = mapped_property(original)
            && !ty.holds_undefined()
            && assignable_rec(&property, expected, types, seen)
        {
            return true;
        }
        // A narrowed form (`Extract<T, string | undefined> & {}`) also holds
        // only the members of its apparent type that the narrowing admits.
        // So does a narrowed bounded type parameter: `T & {}` with `T extends
        // string | null` holds only `string`.
        let bounded_parameter = matches!(original.without_aliases(), Type::GenericParam { id, .. }
            if types.generic_constraints.contains_key(id));
        if (original.is_type_operator() || bounded_parameter) && !mentions_signature_var(original) {
            let bound = upper_bound(original, types, 0);
            let admitted: Vec<Type> = super::utility_types::members(&bound)
                .into_iter()
                .filter(|member| assignable_rec(member, ty, types, seen))
                .collect();
            if !admitted.is_empty() && assignable_rec(&Type::union(admitted), expected, types, seen)
            {
                return true;
            }
        }
        return assignable_rec(original, expected, types, seen)
            || assignable_rec(ty, expected, types, seen);
    }
    if let Type::GenericParam { id, .. } = actual.peel() {
        if matches!(expected.peel(), Type::GenericParam { id: other, .. } if id == other) {
            return true;
        }
        if let Some(constraint) = types.generic_constraints.get(id) {
            let pair = (actual.clone(), expected.clone());
            if seen.contains(&pair) {
                return false;
            }
            // A chain of distinct parameters can be deep even when every
            // individual bound is shallow. Bound the proof before recursing.
            if seen.len() >= crate::compiler_limits::MAX_TYPE_RESOLUTION_DEPTH as usize {
                types.limits.record(crate::type_size::TypeTooLarge::Depth);
                return false;
            }
            seen.push(pair);
            let result = assignable_rec(constraint, expected, types, seen);
            seen.pop();
            if result {
                return true;
            }
        }
    }
    if same_alias_instance(actual, expected) {
        return true;
    }
    // Before a union target is split: `Uppercase<T>` fits `"FOO" | "BAR"`
    // through its bound only as a whole.
    if let Some(related) = relate_type_operators(actual, expected, types, seen) {
        return related;
    }
    // Keep named pairs on the active proof path: expanding a back-edge can
    // change its inline spelling without changing the alias instantiation.
    if seen
        .iter()
        .any(|(a, e)| same_alias_instance(actual, a) && same_alias_instance(expected, e))
    {
        return true;
    }
    // Distribute unions, including named unions, before expanding back-edges.
    // Non-union members retain their names and can reuse the active comparison.
    if let Type::Union(ms) = actual.peel() {
        return ms.iter().all(|m| assignable_rec(m, expected, types, seen));
    }
    if let Type::Union(ms) = expected.peel() {
        // An enum is the union of its members, so `E` fits `E.A | E.B` when
        // that names every member.
        if let Some(members) = enum_as_member_union(actual, ms, types) {
            return members
                .iter()
                .all(|member| ms.iter().any(|m| assignable_rec(member, m, types, seen)));
        }
        return ms.iter().any(|m| assignable_rec(actual, m, types, seen));
    }
    if drops_readonly(actual, expected) || readonly_meets_mutator(actual, expected, types) {
        return false;
    }
    if let Some(chain) = super::utility_types::symbolic_chain(expected) {
        return fits_symbolic_mapping(&chain, actual);
    }
    if matches!(actual, Type::Alias { .. }) || matches!(expected, Type::Alias { .. }) {
        let pair = (actual.clone(), expected.clone());
        seen.push(pair);
        let result = assignable_rec(actual.peel(), expected.peel(), types, seen);
        seen.pop();
        return result;
    }
    let actual = actual.peel();
    let expected = expected.peel();
    if actual.is_regexp_match_array() && matches!(expected, Type::Array(_)) {
        return assignable_rec(
            &Type::Array(Box::new(Type::regexp_element())),
            expected,
            types,
            seen,
        );
    }

    if matches!(actual, Type::Error) || matches!(expected, Type::Error) {
        return true;
    }
    if matches!(actual, Type::Never) {
        return true;
    }
    if matches!(
        (actual, expected),
        (Type::Undefined | Type::Void, Type::Void)
    ) {
        return true;
    }
    if matches!(expected, Type::TypeVar(_)) || matches!(actual, Type::TypeVar(_)) {
        return true;
    }
    // Placed after TypeVar so generic fns can receive `unknown`; the wildcard binds T→Unknown, post-substitution sees identity.
    if matches!(expected, Type::Unknown) {
        return true;
    }
    if matches!(actual, Type::Unknown) {
        return false;
    }
    // A recursion back-edge on either side is expanded by name and
    // compared structurally; the `seen` set breaks the cycle. Two
    // back-edges to the same alias with assignable args short-circuit.
    if matches!(actual, Type::AliasRef { .. }) || matches!(expected, Type::AliasRef { .. }) {
        if let (
            Type::AliasRef {
                mangled: ma,
                args: aa,
                ..
            },
            Type::AliasRef {
                mangled: me,
                args: ae,
                ..
            },
        ) = (actual, expected)
            && ma == me
            && aa.len() == ae.len()
            && aa
                .iter()
                .zip(ae.iter())
                .all(|(a, e)| assignable_rec(a, e, types, seen))
        {
            return true;
        }
        let key = (actual.clone(), expected.clone());
        if seen.iter().any(|(a, e)| a == &key.0 && e == &key.1) {
            return true;
        }
        // Different instantiations of a type-growing alias may never repeat
        // a pair. Bound that proof rather than overflowing the compiler stack.
        let recursive_expansions = seen
            .iter()
            .filter(|(actual, expected)| {
                matches!(actual, Type::AliasRef { .. }) || matches!(expected, Type::AliasRef { .. })
            })
            .count();
        if recursive_expansions >= 32 {
            return false;
        }
        seen.push(key);
        let a = expand_alias_ref(actual, types);
        let e = expand_alias_ref(expected, types);
        let result = assignable_rec(&a, &e, types, seen);
        seen.pop();
        return result;
    }
    match (actual, expected) {
        // `{}` holds every value but `null` and `undefined`, as in TypeScript.
        (Type::Null | Type::Undefined | Type::Void, _) if expected.is_empty_object() => false,
        (Type::GenericParam { .. }, _) if expected.is_empty_object() => false,
        (_, _) if expected.is_empty_object() => true,
        // `object` holds every value but the primitives, `null` and `undefined`.
        (_, Type::NonPrimitive) => matches!(
            actual,
            Type::NonPrimitive
                | Type::Object { .. }
                | Type::Array(_)
                | Type::Tuple(_)
                | Type::Readonly(_)
                | Type::InterfaceRef { .. }
                | Type::ClassRef { .. }
                | Type::Function { .. }
                | Type::Uint8Array
        ),
        (Type::NonPrimitive, _) => false,
        (_, Type::Object { .. } | Type::InterfaceRef { .. } | Type::ClassRef { .. })
            if types.strict_subtype && is_primitive(actual) =>
        {
            false
        }
        (Type::GenericParam { id: a, .. }, Type::GenericParam { id: b, .. }) => a == b,
        (Type::GenericParam { .. }, _) | (_, Type::GenericParam { .. }) => false,
        // Explicit before the `==` fallback, which would reject `StringLiteral("foo") <: String`.
        (
            Type::TemplateLiteral {
                parts: actual_parts,
                substitutions: actual_holes,
            },
            Type::TemplateLiteral {
                parts: expected_parts,
                substitutions: expected_holes,
            },
        ) => {
            if actual_parts == expected_parts && actual_holes.len() == expected_holes.len() {
                actual_holes
                    .iter()
                    .zip(expected_holes)
                    .all(|(actual, expected)| {
                        matches!(expected.peel(), Type::String)
                            || assignable_rec(actual, expected, types, seen)
                    })
            } else if expected_holes.len() == 1
                && matches!(expected_holes.first().map(Type::peel), Some(Type::String))
            {
                actual_parts
                    .first()
                    .zip(expected_parts.first())
                    .is_some_and(|(a, b)| a.starts_with(b))
                    && actual_parts
                        .last()
                        .zip(expected_parts.last())
                        .is_some_and(|(a, b)| a.ends_with(b))
            } else {
                false
            }
        }
        (Type::StringLiteral(value), Type::TemplateLiteral { .. }) => {
            crate::property_keys::matches_name(expected, value, types.limits)
        }
        (Type::StringLiteral(a), Type::StringLiteral(b)) => a == b,
        (
            Type::TemplateLiteral { .. } | Type::StringLiteral(_) | Type::StringEnum { .. },
            Type::String,
        ) => true,
        (Type::String, Type::StringLiteral(_)) => false,
        (Type::NumberLiteral(a), Type::NumberLiteral(b)) => a == b,
        (Type::NumberLiteral(_) | Type::NumberEnum { .. }, Type::Number) => true,
        (Type::Number, Type::NumberLiteral(_)) => false,
        (Type::BigIntLiteral(a), Type::BigIntLiteral(b)) => a == b,
        (Type::BigIntLiteral(_), Type::BigInt) => true,
        (Type::BigInt, Type::BigIntLiteral(_)) => false,
        (Type::BooleanLiteral(a), Type::BooleanLiteral(b)) => a == b,
        (Type::BooleanLiteral(_), Type::Boolean) => true,
        (Type::Boolean, Type::BooleanLiteral(_)) => false,
        // An enum's identity is its declaration: an aliased import
        // (`import { E as G }`) names the same enum under another `name`. An
        // enum member literal type `E.A` is its enum's and its value's.
        (
            Type::NumberEnum {
                mangled: ma,
                member: am,
                ..
            },
            Type::NumberEnum {
                mangled: me,
                member: em,
                ..
            },
        ) => ma == me && enum_member_fits(am.as_ref(), em.as_ref()),
        (
            Type::StringEnum {
                mangled: ma,
                member: am,
                ..
            },
            Type::StringEnum {
                mangled: me,
                member: em,
                ..
            },
        ) => ma == me && enum_member_fits(am.as_ref(), em.as_ref()),
        (
            Type::NumberEnum {
                member: Some(member),
                ..
            },
            Type::NumberLiteral(value),
        ) => member.value == *value,
        (
            Type::StringEnum {
                member: Some(member),
                ..
            },
            Type::StringLiteral(value),
        ) => member.value == *value,
        // TypeScript lets a number literal stand for the numeric member that
        // holds its value, but no string literal for a string member.
        (
            Type::NumberLiteral(value),
            Type::NumberEnum {
                member: Some(member),
                ..
            },
        ) => member.value == *value,
        (Type::Array(ae), Type::Array(ee)) => assignable_rec(ae, ee, types, seen),
        (Type::Tuple(aa), Type::Tuple(ae)) => {
            aa.required_len() >= ae.required_len()
                && aa.len() <= ae.len()
                && aa
                    .iter()
                    .zip(ae.iter())
                    .all(|(a, e)| assignable_rec(a, e, types, seen))
        }
        (Type::Tuple(aa), Type::Array(ee)) => aa.iter().all(|a| assignable_rec(a, ee, types, seen)),
        (
            Type::InterfaceRef {
                mangled: ma,
                args: aa,
                name: na,
                ..
            },
            Type::InterfaceRef {
                mangled: me,
                args: ae,
                ..
            },
        ) => {
            // Measured variances decide first, as in tsc: comparing the members
            // of a recursive interface instead would expand ever larger
            // instantiations (`I0<I1<T, U>, T>`). Arguments that fail only
            // where the members may still accept them fall back to plain
            // covariance, then to the members.
            if ma == me {
                match args_relate_at_variances(ma, na, aa, ae, types, seen) {
                    Some(ArgsRelation::Related) => return true,
                    Some(ArgsRelation::Unrelated) => return false,
                    Some(ArgsRelation::MembersDecide) | None => {}
                }
                if args_relate_covariantly(aa, ae, types, seen) {
                    return true;
                }
            }
            satisfies_structurally(actual, expected, types, seen)
        }
        // Classes are nominal: assignable only up the `extends` chain. Generic
        // classes additionally compare args pairwise, at each type parameter's
        // variance (mutable fields count as covariant, as in TypeScript).
        (
            Type::ClassRef {
                mangled: ma,
                args: aa,
                ..
            },
            Type::ClassRef {
                mangled: me,
                name: ne,
                args: ae,
                ..
            },
        ) => {
            // A subclass relates through the args its extends chain applies to
            // the ancestor: `NumBox extends Box<number>` reaches `Box` at
            // `[number]`, so it satisfies `Box<number>` but not `Box<string>`.
            let actual_at_expected = if ma == me {
                Some(aa.clone())
            } else {
                types.class_args_at_ancestor(ma, aa, me)
            };
            match actual_at_expected {
                // Classes are nominal and have no member fallback, so arguments
                // the members might accept still leave them unrelated.
                Some(at) => match args_relate_at_variances(me, ne, &at, ae, types, seen) {
                    Some(relation) => relation == ArgsRelation::Related,
                    None => args_relate_covariantly(&at, ae, types, seen),
                },
                None => false,
            }
        }
        // A class instance satisfies an interface structurally (its `implements`
        // contract and ordinary duck typing) through its public member shape.
        (
            Type::ClassRef {
                mangled: ma,
                args: aa,
                ..
            },
            Type::InterfaceRef {
                mangled: me,
                name: ne,
                args: ae,
                ..
            },
        ) => {
            // A class instance is no function: it has no call or construct
            // signature, as in TypeScript.
            if is_host_backed_interface(expected) || interface_has_signature(expected, types) {
                return false;
            }
            let (Some(class_form), Some(iface_form)) = (
                types.class_full_form(ma, aa),
                types.interface_full_form(me, ne, ae),
            ) else {
                return false;
            };
            if !class_members_fit(&class_form, &types.index_signatures(expected), types, seen) {
                return false;
            }
            if weak_type_rejects(&class_form, &iface_form) {
                return false;
            }
            iface_form
                .iter()
                .all(|(member, exp)| match class_form.get(member) {
                    Some(act) => {
                        !types.class_property_is_write_only(ma, aa, member)
                            && (exp.optional || !act.optional)
                            && (if exp.method {
                                types
                                    .method_relation(actual, expected, member, seen)
                                    .unwrap_or_else(|| {
                                        types.class_member_assignable(
                                            ma,
                                            aa,
                                            member,
                                            &act.read_ty(),
                                            &exp.read_ty(),
                                            seen,
                                        )
                                    })
                            } else {
                                types.class_member_assignable(
                                    ma,
                                    aa,
                                    member,
                                    &act.read_ty(),
                                    &exp.read_ty(),
                                    seen,
                                )
                            })
                    }
                    None => {
                        exp.optional
                            || exp.method
                                && types.method_relation(actual, expected, member, seen)
                                    == Some(true)
                    }
                })
                && types.required_methods_satisfied(actual, expected, seen)
        }
        (
            Type::ClassRef { mangled, args, .. },
            Type::Object {
                fields,
                index,
                more_indexes,
            },
        ) => {
            let Some(class_form) = types.class_full_form(mangled, args) else {
                return false;
            };
            let signatures: Vec<_> = crate::stored_index_signatures(index, more_indexes)
                .cloned()
                .collect();
            if !class_members_fit(&class_form, &signatures, types, seen) {
                return false;
            }
            if weak_type_rejects(&class_form, fields) {
                return false;
            }
            fields
                .iter()
                .all(|(member, expected)| match class_form.get(member) {
                    Some(actual) => {
                        !types.class_property_is_write_only(mangled, args, member)
                            && (expected.optional || !actual.optional)
                            && types.class_member_assignable(
                                mangled,
                                args,
                                member,
                                &actual.read_ty(),
                                &expected.read_ty(),
                                seen,
                            )
                    }
                    None => expected.optional,
                })
        }
        // An interface value is not nominally a class.
        (Type::InterfaceRef { .. }, Type::ClassRef { .. }) => false,
        // A value of an interface with a call signature is a function, a
        // hybrid's included: it stands for a function type its call signature
        // fits, as in TypeScript.
        (Type::InterfaceRef { .. }, Type::Function { .. })
            if !is_host_backed_interface(actual)
                && types
                    .signature_of(actual, Signature::Call)
                    .is_some_and(|call| assignable_rec(&call, expected, types, seen)) =>
        {
            true
        }
        // Non-interface values (strings, arrays, …) satisfy an interface
        // through their own interface form — `string` is `Iterable<string>`
        // because `String` declares `iterator()`.
        (_, Type::InterfaceRef { .. }) if satisfies_structurally(actual, expected, types, seen) => {
            true
        }
        (
            Type::Function {
                params: pa,
                ret: ra,
                predicate: predicate_a,
                has_rest: rest_a,
                optional: optional_a,
            },
            Type::Function {
                params: pe,
                ret: re,
                predicate: predicate_e,
                has_rest: rest_e,
                optional: optional_e,
            },
        ) => {
            if types.strict_subtype
                && !strict_arity_fits(
                    Arity {
                        params: pa,
                        optional: *optional_a,
                        rest: *rest_a,
                    },
                    Arity {
                        params: pe,
                        optional: *optional_e,
                        rest: *rest_e,
                    },
                    |a, e| assignable_rec(a, e, types, seen),
                )
            {
                return false;
            }
            // A rest function can't stand for one whose own rest arguments
            // it would have to unpack.
            if *rest_e && !rest_a {
                return false;
            }
            // Predicate→non-predicate: ok (info dropped). Non-predicate→predicate: rejected (can't manufacture narrowing metadata).
            // A guard and an assertion never stand for each other; an
            // assertion returns `void`, so the return check below lets it
            // stand for a `void` function, as in tsc.
            let predicate_ok = match (predicate_a, predicate_e) {
                (Some(a), Some(e)) => {
                    a.kind == e.kind
                        && a.parameter_index == e.parameter_index
                        && assignable_rec(&a.asserted_type, &e.asserted_type, types, seen)
                }
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (None, None) => true,
            };
            let params_ok = if *rest_a && !rest_e {
                rest_function_accepts(pa, pe, |e, a| assignable_rec(e, a, types, seen))
            } else {
                // Trailing optional parameters may go unpassed by the caller.
                let required = if *rest_a {
                    pa.len()
                } else {
                    pa.len().saturating_sub(*optional_a)
                };
                Type::function_arity_fits(required, pe.len(), *rest_a)
                    && pa.iter().zip(pe.iter()).enumerate().all(|(i, (a, e))| {
                        let both_rest = *rest_a && i + 1 == pa.len() && i + 1 == pe.len();
                        if both_rest {
                            assignable_rec(
                                e.rest_array_ignoring_readonly(),
                                a.rest_array_ignoring_readonly(),
                                types,
                                seen,
                            )
                        } else {
                            assignable_rec(e, a, types, seen)
                        }
                    })
            };
            predicate_ok
                && params_ok
                && (re.is_void()
                    || (ra.is_void() && matches!(re.peel(), Type::TypeVar(_) | Type::Unknown))
                    || assignable_rec(ra, re, types, seen))
        }
        // `readonly` is shallow (TS-faithful): it gates direct writes, not
        // assignability. Object width subtyping stays covariant per field.
        (
            Type::Object {
                fields: a_fields,
                index: a_index,
                more_indexes: a_more,
            },
            Type::Object {
                fields: e_fields,
                index: e_index,
                more_indexes: e_more,
            },
        ) => {
            let actual_indexes: Vec<_> = crate::stored_index_signatures(a_index, a_more)
                .cloned()
                .collect();
            for expected_index in crate::stored_index_signatures(e_index, e_more) {
                if !a_fields
                    .iter()
                    .filter(|(name, _)| {
                        crate::property_keys::matches_name(&expected_index.key, name, types.limits)
                    })
                    .all(|(_, f)| assignable_rec(&f.ty, &expected_index.value, types, seen))
                {
                    return false;
                }
                if !super::records::source_indexes_fit(
                    &actual_indexes,
                    expected_index,
                    types,
                    |value| assignable_rec(value, &expected_index.value, types, seen),
                ) {
                    return false;
                }
            }
            if e_index.is_none() && weak_type_rejects(a_fields, e_fields) {
                return false;
            }
            // `{a: T|null}` into `{a?: T}` is still rejected — `assignable(T|null, T)` fails.
            e_fields.iter().all(|(k, e_field)| match a_fields.get(k) {
                Some(a_field) => {
                    if a_field.optional && !e_field.optional {
                        return false;
                    }
                    assignable_rec(&a_field.read_ty(), &e_field.read_ty(), types, seen)
                }
                None => e_field.optional && !types.strict_subtype,
            })
        }
        // Structural compatibility for *data-only* interfaces, both directions:
        // expand the interface to its property shape and reuse the `(Object,
        // Object)` width rule. Method-bearing interfaces expand to `None` and
        // fall through to rejection — a plain object has no vtable, so a later
        // method call would trap. The `seen` guard breaks recursive interfaces
        // coinductively (mirrors the `AliasRef` arm above).
        (Type::Object { .. }, Type::InterfaceRef { .. })
        | (Type::InterfaceRef { .. }, Type::Object { .. }) => {
            if is_host_backed_interface(expected) {
                return false;
            }
            let expanded_pair = if matches!(expected, Type::InterfaceRef { .. }) {
                expand_interface_data_shape(expected, types).map(|o| (actual.clone(), o))
            } else {
                expand_interface_data_shape(actual, types).map(|o| (o, expected.clone()))
            };
            let Some((a, e)) = expanded_pair else {
                return false;
            };
            let key = (actual.clone(), expected.clone());
            if seen.iter().any(|(sa, se)| sa == &key.0 && se == &key.1) {
                return true;
            }
            seen.push(key);
            let result = assignable_rec(&a, &e, types, seen);
            seen.pop();
            result && types.required_methods_satisfied(actual, expected, seen)
        }
        _ => actual == expected,
    }
}

/// Whether a value of `ty` is a primitive, which tsc's strict subtype relation
/// keeps apart from object types even where it is assignable to one.
fn is_primitive(ty: &Type) -> bool {
    matches!(
        ty,
        Type::String
            | Type::StringLiteral(_)
            | Type::StringEnum { .. }
            | Type::Number
            | Type::NumberLiteral(_)
            | Type::NumberEnum { .. }
            | Type::BigInt
            | Type::BigIntLiteral(_)
            | Type::Boolean
            | Type::BooleanLiteral(_)
            | Type::Null
            | Type::Undefined
            | Type::Void
    )
}

/// The parameter shape of a function type that arity rules read.
#[derive(Clone, Copy)]
struct Arity<'a> {
    params: &'a [Type],
    /// Trailing fixed parameters a caller may omit.
    optional: usize,
    rest: bool,
}

impl Arity<'_> {
    fn required(self) -> usize {
        super::expr::required_count_of_function_type(self.params, self.optional, self.rest)
    }
}

/// tsc's strict arity rule for a function of `actual`'s shape to be a subtype
/// of one of `expected`'s: it takes no parameter the other lacks, unless that
/// one has a rest parameter, and makes no argument optional that the other
/// requires at a type `actual` accepts in turn (`related`).
fn strict_arity_fits(
    actual: Arity<'_>,
    expected: Arity<'_>,
    mut related: impl FnMut(&Type, &Type) -> bool,
) -> bool {
    if !expected.rest && (actual.rest || actual.params.len() > expected.params.len()) {
        return false;
    }
    let omittable_only_in_actual = actual.required()..expected.required();
    omittable_only_in_actual.into_iter().all(|position| {
        match (actual.params.get(position), expected.params.get(position)) {
            (Some(actual), Some(expected)) => !related(actual, expected),
            _ => true,
        }
    })
}

/// How one instantiation's type arguments relate to another's at the measured
/// variances.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ArgsRelation {
    Related,
    Unrelated,
    /// Every failing argument may still fit through the members: a `void`
    /// at a covariant parameter (tsc's `hasCovariantVoidArgument`, so a
    /// `Task<number>` serves as a `Task<void>`), or a type parameter still
    /// being inferred (`Sink<T>` for a `Sink<void>`).
    MembersDecide,
}

/// How one instantiation's type arguments relate to another's, for the generic
/// class or interface `mangled`, at each parameter's variance. `None` when the
/// variances couldn't be measured in full.
fn args_relate_at_variances(
    mangled: &MangledName,
    name: &str,
    actual_args: &[Type],
    expected_args: &[Type],
    types: TypeResolver,
    seen: &mut Vec<(Type, Type)>,
) -> Option<ArgsRelation> {
    if actual_args.len() != expected_args.len() {
        return Some(ArgsRelation::Unrelated);
    }
    if actual_args == expected_args {
        return Some(ArgsRelation::Related);
    }
    let variances = types.settled_variances(mangled, name, actual_args.len())?;
    let mut relation = ArgsRelation::Related;
    for ((actual, expected), variance) in actual_args.iter().zip(expected_args).zip(variances) {
        if variance.relates(actual, expected, |a, e| assignable_rec(a, e, types, seen)) {
            continue;
        }
        let members_may_accept = (variance == Variance::Covariant
            && matches!(expected, Type::Void))
            || super::expr::type_contains_type_var(expected);
        if !members_may_accept {
            return Some(ArgsRelation::Unrelated);
        }
        relation = ArgsRelation::MembersDecide;
    }
    Some(relation)
}

/// Whether each type argument is assignable to the one at its position.
fn args_relate_covariantly(
    actual_args: &[Type],
    expected_args: &[Type],
    types: TypeResolver,
    seen: &mut Vec<(Type, Type)>,
) -> bool {
    actual_args.len() == expected_args.len()
        && actual_args
            .iter()
            .zip(expected_args)
            .all(|(actual, expected)| assignable_rec(actual, expected, types, seen))
}

/// Whether `actual` is a `readonly` array or tuple, or a `ReadonlyMap` or
/// `ReadonlySet`, and `expected` a mutable one. The element types still decide
/// assignability everywhere else: `readonly` is shallow and otherwise
/// covariant, like the collection it wraps.
pub(crate) fn drops_readonly(actual: &Type, expected: &Type) -> bool {
    actual.is_readonly_array()
        && !expected.is_readonly_array()
        && match expected.peel() {
            Type::Array(_) | Type::Tuple(_) => true,
            Type::InterfaceRef { mangled, .. } => {
                *mangled == crate::mangle::prelude("Map")
                    || *mangled == crate::mangle::prelude("Set")
            }
            _ => false,
        }
}

fn same_alias_instance(left: &Type, right: &Type) -> bool {
    if left == right {
        return true;
    }
    let (Some((left_name, left_args)), Some((right_name, right_args))) =
        (alias_identity(left), alias_identity(right))
    else {
        return false;
    };
    left_name == right_name
        && left_args.len() == right_args.len()
        && left_args
            .iter()
            .zip(right_args)
            .all(|(left, right)| same_alias_instance(left, right))
}

pub(super) fn alias_identity(ty: &Type) -> Option<(&MangledName, &[Type])> {
    match ty {
        Type::Alias { mangled, args, .. } | Type::AliasRef { mangled, args, .. } => {
            Some((mangled, args))
        }
        _ => None,
    }
}

/// A class's private members, keyed by the class that declares each and its
/// name, as [`TypeResolver::class_private_members`] returns them.
pub(super) type PrivateMembers = BTreeMap<(MangledName, String), Type>;

/// Expand a recursion back-edge to the alias's underlying body (args
/// substituted), by name. Returns the input unchanged for non-`AliasRef`
/// or an unresolvable name.
pub(crate) fn expand_alias_ref(ty: &Type, types: TypeResolver) -> Type {
    let Type::AliasRef {
        mangled,
        name,
        args,
        ..
    } = ty
    else {
        return ty.clone();
    };
    // A utility's alias body is an application of itself: its evaluation is
    // the body to compare against.
    if super::utility_types::utility(mangled).is_some() {
        return super::type_aliases::alias_ref_body(mangled, name, args, types);
    }
    let Some(sym) = types.lookup(mangled, name) else {
        return ty.clone();
    };
    let TypeKind::Alias {
        generics, ty: body, ..
    } = &sym.kind
    else {
        return ty.clone();
    };
    if generics.is_empty() {
        body.clone()
    } else {
        let sub = crate::typechecker::type_param_substitution::TypeParamSubstitution::from_pairs(
            generics, args,
        );
        sub.apply_or_record(body, types.limits)
    }
}

/// Whether `actual` is a `readonly` array, `ReadonlyMap` or `ReadonlySet` and
/// `expected` an interface that requires one of the mutating methods the
/// readonly view leaves out, as tsc reports.
fn readonly_meets_mutator(actual: &Type, expected: &Type, types: TypeResolver) -> bool {
    if !actual.is_readonly_array() || expected.is_readonly_array() {
        return false;
    }
    let Some((mangled, _package, name, args)) = expected.interface_routing() else {
        return false;
    };
    types
        .interface_full_form(&mangled, name, &args)
        .is_some_and(|form| {
            form.iter()
                .any(|(member, field)| !field.optional && is_mutating_member(member))
        })
}

/// A method a `readonly` array, `ReadonlyMap` or `ReadonlySet` does not have,
/// so a readonly value can't satisfy an interface that requires one.
fn is_mutating_member(name: &str) -> bool {
    super::expr::is_mutating_array_method(name)
        || matches!(name, "set" | "add" | "delete" | "clear")
}

/// TS-style structural interface satisfaction: every member the expected
/// interface declares exists on the actual type's interface form (via
/// [`Type::interface_routing`]) with an assignable type. The nominal fast
/// path stays first; this is the fallback that makes interfaces structural.
fn satisfies_structurally(
    actual: &Type,
    expected: &Type,
    types: TypeResolver,
    seen: &mut Vec<(Type, Type)>,
) -> bool {
    // Opaque backing structs cannot satisfy reads through an object shape.
    // Same-interface assignments have already taken the nominal fast path.
    if is_host_backed_interface(expected) || is_host_backed_interface(actual) {
        return false;
    }
    let key = (actual.clone(), expected.clone());
    if seen.contains(&key) {
        // Coinductive: an in-progress pair counts as satisfied.
        return true;
    }
    if expanding_interface_pair(actual, expected, seen) {
        return true;
    }
    let Some((me, _pe, ne, ae)) = expected.interface_routing() else {
        return false;
    };
    let Some(expected_form) = types.interface_full_form(&me, ne, &ae) else {
        return false;
    };
    let actual_signature = |which: Signature| match actual {
        Type::Function { .. } => (which == Signature::Call).then(|| actual.clone()),
        _ => types.signature_of(actual, which),
    };
    let actual_form = if matches!(actual, Type::Function { .. }) {
        if has_required_generic_method(&me, ne, types) {
            return false;
        }
        BTreeMap::new()
    } else {
        let Some((ma, _pa, na, aa)) = actual.interface_routing() else {
            return false;
        };
        let Some(actual_form) = types.interface_full_form(&ma, na, &aa) else {
            return false;
        };
        actual_form
    };
    let expected_call = types.interface_signature(&me, ne, &ae, Signature::Call);
    let expected_construct = types.interface_signature(&me, ne, &ae, Signature::Construct);
    // A signature is a member TypeScript's weak-type check counts.
    if expected_call.is_none()
        && expected_construct.is_none()
        && actual_signature(Signature::Call).is_none()
        && weak_type_rejects(&actual_form, &expected_form)
    {
        return false;
    }
    let expected_indexes = types.index_signatures(expected);
    if !expected_indexes.is_empty() {
        if !matches!(
            actual.peel(),
            Type::Object { .. } | Type::ClassRef { .. } | Type::InterfaceRef { .. }
        ) {
            return false;
        }
        let actual_indexes = types.index_signatures(actual);
        seen.push(key.clone());
        let compatible = expected_indexes.iter().all(|index| {
            actual_form
                .iter()
                .filter(|(name, _)| {
                    crate::property_keys::matches_name(&index.key, name, types.limits)
                })
                .all(|(_, field)| assignable_rec(&field.ty, &index.value, types, seen))
                && super::records::source_indexes_fit(&actual_indexes, index, types, |value| {
                    assignable_rec(value, &index.value, types, seen)
                })
        });
        seen.pop();
        if !compatible {
            return false;
        }
    }
    seen.push(key);
    let signatures_fit = [
        (Signature::Call, &expected_call),
        (Signature::Construct, &expected_construct),
    ]
    .into_iter()
    .all(|(which, expected)| match expected {
        None => true,
        Some(expected) => actual_signature(which)
            .is_some_and(|actual| assignable_rec(&actual, expected, types, seen)),
    });
    let ok = signatures_fit
        && expected_form
            .iter()
            .all(|(member, exp)| match actual_form.get(member) {
                Some(act) => {
                    (exp.optional || !act.optional)
                        && (if exp.method {
                            types
                                .method_relation(actual, expected, member, seen)
                                .unwrap_or_else(|| {
                                    assignable_rec(&act.read_ty(), &exp.read_ty(), types, seen)
                                })
                        } else {
                            assignable_rec(&act.read_ty(), &exp.read_ty(), types, seen)
                        })
                }
                None => {
                    exp.optional
                        || exp.method
                            && types.method_relation(actual, expected, member, seen) == Some(true)
                }
            });
    seen.pop();
    ok && types.required_methods_satisfied(actual, expected, seen)
}

/// Expansive recursion produces fresh instantiations forever, so exact-pair
/// coinduction cannot terminate it. Require growth on at least one side of the same
/// named comparison; explicitly nested, shrinking arguments still get checked.
fn expanding_interface_pair(actual: &Type, expected: &Type, seen: &[(Type, Type)]) -> bool {
    let (
        Type::InterfaceRef {
            mangled: actual_name,
            args: actual_args,
            ..
        },
        Type::InterfaceRef {
            mangled: expected_name,
            args: expected_args,
            ..
        },
    ) = (actual, expected)
    else {
        return false;
    };
    let mut previous = (
        type_argument_depth(actual_args),
        type_argument_depth(expected_args),
    );
    let mut expansions = 0;
    for (actual, expected) in seen.iter().rev() {
        let (
            Type::InterfaceRef {
                mangled: ancestor_actual_name,
                args: ancestor_actual_args,
                ..
            },
            Type::InterfaceRef {
                mangled: ancestor_expected_name,
                args: ancestor_expected_args,
                ..
            },
        ) = (actual, expected)
        else {
            continue;
        };
        if ancestor_actual_name != actual_name || ancestor_expected_name != expected_name {
            continue;
        }
        let depth = (
            type_argument_depth(ancestor_actual_args),
            type_argument_depth(ancestor_expected_args),
        );
        if depth.0 > previous.0 || depth.1 > previous.1 || depth == previous {
            continue;
        }
        expansions += 1;
        if expansions == 3 {
            return true;
        }
        previous = depth;
    }
    false
}

fn type_argument_depth(args: &[Type]) -> usize {
    args.iter().map(type_nesting_depth).max().unwrap_or(0)
}

fn type_nesting_depth(ty: &Type) -> usize {
    1 + match ty {
        Type::Array(element) | Type::Readonly(element) => type_nesting_depth(element),
        Type::InterfaceRef { args, .. }
        | Type::ClassRef { args, .. }
        | Type::AliasRef { args, .. }
        | Type::Alias { args, .. } => type_argument_depth(args),
        Type::Tuple(elements) => type_argument_depth(elements),
        Type::Union(elements) => type_argument_depth(elements),
        Type::Function { params, ret, .. } => {
            type_argument_depth(params).max(type_nesting_depth(ret))
        }
        Type::Refined { original, ty } => type_nesting_depth(original).max(type_nesting_depth(ty)),
        Type::Object {
            fields,
            index,
            more_indexes,
        } => fields
            .values()
            .map(|field| type_nesting_depth(&field.ty))
            .chain(
                crate::stored_index_signatures(index, more_indexes)
                    .map(|i| type_nesting_depth(&i.value)),
            )
            .max()
            .unwrap_or(0),
        _ => 0,
    }
}

/// TS weak-type rule: an all-optional target is vacuously satisfied by
/// member-by-member checks, which would let `number` satisfy `{ years?: number }`
/// through its `Number` interface form (and codegen would then put an f64 in
/// a ref slot — invalid Wasm), or let `{ p: number }` stand for `{ q?: string }`
/// while it holds a `q` of another type. Require at least one member in common.
/// An empty actual form stays assignable, mirroring TS's `{}`-source exemption.
pub(super) fn weak_type_rejects(
    actual_form: &BTreeMap<String, ObjectField>,
    expected_form: &BTreeMap<String, ObjectField>,
) -> bool {
    !expected_form.is_empty()
        && expected_form.values().all(|f| f.optional)
        && !actual_form.is_empty()
        && !expected_form.keys().any(|k| actual_form.contains_key(k))
}

pub(super) fn has_required_generic_method(
    mangled: &MangledName,
    name: &str,
    types: TypeResolver,
) -> bool {
    let Some(TypeKind::Interface { methods, .. }) = types.lookup(mangled, name).map(|s| &s.kind)
    else {
        return false;
    };
    methods.values().any(|sig| {
        !sig.optional
            && sig
                .signatures()
                .any(|signature| !signature.generics.is_empty())
    })
}

/// Whether every member of a class instance fits every one of `signatures`:
/// a class has no index signature of its own to say which keys it holds.
fn class_members_fit(
    class_form: &BTreeMap<String, ObjectField>,
    signatures: &[crate::IndexSignature],
    types: TypeResolver,
    seen: &mut Vec<(Type, Type)>,
) -> bool {
    signatures.iter().all(|index| {
        class_form
            .values()
            .all(|field| assignable_rec(&field.ty, &index.value, types, seen))
    })
}

/// `Some` when a function can inhabit `ty`, holding its call signature if it
/// declares one: `ty` is a non-host-backed
/// `InterfaceRef` with no index signature, no construct signature, no required
/// generic methods, and only optional named members — i.e., an interface whose
/// shape a function or a callable sub-interface can inhabit.
pub(super) fn function_inhabitable_call_signature(
    ty: &Type,
    types: TypeResolver,
) -> Option<Option<Type>> {
    let ty = ty.peel();
    if is_host_backed_interface(ty) || types.has_index_signature(ty) {
        return None;
    }
    let Type::InterfaceRef {
        mangled,
        name,
        args,
        ..
    } = ty
    else {
        return None;
    };
    if has_required_generic_method(mangled, name, types)
        || types
            .interface_signature(mangled, name, args, Signature::Construct)
            .is_some()
    {
        return None;
    }
    let members = types.interface_full_form(mangled, name, args)?;
    members
        .values()
        .all(|field| field.optional)
        .then(|| types.interface_signature(mangled, name, args, Signature::Call))
}

/// Which of an interface's signatures [`TypeResolver::interface_signature`]
/// reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Signature {
    Call,
    Construct,
}

/// Whether `ty` is an interface with a call or construct signature: every
/// value of it is callable, so `typeof` answers `"function"` for it.
pub(super) fn interface_has_signature(ty: &Type, types: TypeResolver) -> bool {
    matches!(ty.peel(), Type::InterfaceRef { .. })
        && (types.signature_of(ty, Signature::Call).is_some()
            || types.signature_of(ty, Signature::Construct).is_some())
}

/// Expand a call-signature `InterfaceRef` to its call signature's
/// `Type::Function` when functions structurally satisfy the interface (no
/// host backing, index signature, or required non-call members).
pub(crate) fn expand_interface_call_signature(ty: &Type, types: TypeResolver) -> Option<Type> {
    function_inhabitable_call_signature(ty, types).flatten()
}

/// Whether a value typed as `ty` may hold a function at run time through
/// structural interface assignability (including via a callable sub-interface),
/// so `JSON.stringify` may omit it and `typeof` may observe `"function"`.
pub(super) fn interface_may_hold_function(ty: &Type, types: TypeResolver) -> bool {
    function_inhabitable_call_signature(ty, types).is_some()
}

/// Expand a **data-only** (method-free) `InterfaceRef` to the structural
/// `Type::Object` of its properties. Returns `None` for non-interfaces,
/// method-bearing interfaces, or unresolvable names — callers treat `None` as
/// "not structurally assignable".
pub(crate) fn expand_interface_data_shape(ty: &Type, types: TypeResolver) -> Option<Type> {
    let Type::InterfaceRef {
        mangled,
        name,
        args,
        ..
    } = ty
    else {
        return None;
    };
    let (index, more_indexes) = crate::split_index_signatures(types.index_signatures(ty));
    Some(Type::Object {
        more_indexes,
        index,
        fields: types.interface_data_shape(mangled, name, args)?,
    })
}

/// A deferred utility application, `keyof T` or `T[K]` on either side. Like a
/// type parameter they are wildcards while they mention a signature
/// `TypeVar`. Otherwise the rules apply in order:
/// 1. trivial pairs (`never`, an every-value target, identity, a union target
///    holding the form);
/// 2. the keys of a mapped utility on either side ([`mapped_keys`]), then
///    pairs of the same shape ([`relate_same_shape`]);
/// 3. a subset of its operand as the source ([`relate_subset_of_operand`]);
/// 4. a property of a mapped utility as the source ([`mapped_property`]);
/// 5. a form as the target ([`relate_into_form`]), which holds only what the
///    identities tsc proves for any `T` put into it;
/// 6. a form as the source ([`relate_out_of_form`]), by its base constraint.
///
/// `None` when neither side is such a form.
fn relate_type_operators(
    actual: &Type,
    expected: &Type,
    types: TypeResolver,
    seen: &mut Vec<(Type, Type)>,
) -> Option<bool> {
    let (actual, expected) = (actual.without_aliases(), expected.without_aliases());
    let actual_form = actual.is_type_operator();
    let expected_form = expected.is_type_operator();
    if !actual_form && !expected_form {
        return None;
    }
    if (actual_form && mentions_signature_var(actual))
        || (expected_form && mentions_signature_var(expected))
    {
        return Some(true);
    }
    if matches!(actual.peel(), Type::Error | Type::Never)
        || matches!(expected.peel(), Type::Error | Type::Unknown)
        || expected.holds_every_value()
        || actual == expected
    {
        return Some(true);
    }
    if actual_form
        && let Type::Union(members) = expected.peel()
        && members
            .iter()
            .any(|member| assignable_rec(actual, member, types, seen))
    {
        return Some(true);
    }
    // The keys of a mapped utility are its key type, on either side. A
    // concrete key outside it may still be one of the bound's keys
    // (`"b"` in `keyof Omit<T, "a">`), which `relate_into_form` tries.
    if let Some(keys) = mapped_keys(expected) {
        let fits = assignable_rec(actual, &keys, types, seen);
        if fits || actual_form {
            return Some(fits);
        }
    }
    if let Some(keys) = mapped_keys(actual) {
        return Some(assignable_rec(&keys, expected, types, seen));
    }
    if let Some(related) = relate_same_shape(actual, expected, types, seen) {
        return Some(related);
    }
    if relate_subset_of_operand(actual, expected, types, seen) {
        return Some(true);
    }
    // A property of `Readonly<T>`, `Required<T>` or `Pick<T, K>` is `T`'s
    // (narrower, for `Required`), and one of `Partial<T>` may be absent, a
    // form target included: `p[k]` on `Pick<T, K>` fits `T[K]`.
    if let Some(property) = mapped_property(actual) {
        let property = match property {
            MappedProperty::Same(property) => property,
            MappedProperty::Optional(property) => Type::union(vec![property, Type::Undefined]),
        };
        return Some(assignable_rec(&property, expected, types, seen));
    }
    if expected_form {
        return Some(relate_into_form(actual, expected, types, seen));
    }
    Some(relate_out_of_form(actual, expected, types, seen))
}

/// Two forms of the same shape. `keyof` against `keyof` (contravariant) and
/// homomorphic wrappings of one `T` (by optionality) decide. `T[K]` against
/// `T[K]` (covariant in both) and one utility on both sides (through its
/// arguments) only prove: `None` when they do not, for the later rules.
fn relate_same_shape(
    actual: &Type,
    expected: &Type,
    types: TypeResolver,
    seen: &mut Vec<(Type, Type)>,
) -> Option<bool> {
    use super::utility_types::{Utility, deferred_application};
    // A narrower type has more keys, and `Partial`, `Readonly` and
    // `Required` keep their operand's keys.
    if let (Type::KeyOf(actual_operand), Type::KeyOf(expected_operand)) = (actual, expected) {
        let (actual_base, expected_base) = (
            homomorphic_base(actual_operand),
            homomorphic_base(expected_operand),
        );
        return Some(
            actual_base == expected_base || assignable_rec(expected_base, actual_base, types, seen),
        );
    }
    if let (
        Type::IndexedAccess {
            object: actual_object,
            index: actual_index,
        },
        Type::IndexedAccess {
            object: expected_object,
            index: expected_index,
        },
    ) = (actual, expected)
        && assignable_rec(actual_object, expected_object, types, seen)
        && assignable_rec(actual_index, expected_index, types, seen)
    {
        return Some(true);
    }
    // tsc relates generic mapped types by their modifiers: `Readonly<T>`
    // fits `Partial<T>`, and `T` fits `Readonly<T>`.
    if let (Some((actual_base, actual_optional)), Some((expected_base, expected_optional))) =
        (homomorphic_wrapping(actual), homomorphic_wrapping(expected))
        && actual_base == expected_base
    {
        return Some(match (actual_optional, expected_optional) {
            (_, Optionality::Optional) => true,
            (Optionality::Optional, _) => false,
            (_, Optionality::Required) => actual_optional == Optionality::Required,
            (_, Optionality::Unchanged) => true,
        });
    }
    // Utilities that preserve assignability: `Uppercase<U>` fits
    // `Uppercase<T>` when `U` fits `T`.
    let (Some((left, left_args)), Some((right, right_args))) =
        (deferred_application(actual), deferred_application(expected))
    else {
        return None;
    };
    if left != right || left_args.len() != right_args.len() {
        return None;
    }
    let covariant = match (left, left_args, right_args) {
        (Utility::Exclude | Utility::Extract, [left, left_filter], [right, right_filter]) => {
            left_filter == right_filter && assignable_rec(left, right, types, seen)
        }
        (
            Utility::Exclude
            | Utility::Extract
            | Utility::Pick
            | Utility::Omit
            | Utility::Parameters,
            _,
            _,
        ) => false,
        _ => left_args
            .iter()
            .zip(right_args)
            .all(|(left, right)| assignable_rec(left, right, types, seen)),
    };
    covariant.then_some(true)
}

/// A subset of its operand (`Readonly`, `Required`, `Exclude`, `Extract`) fits
/// wherever the operand does, a form target included: `Extract<keyof T, "b">`
/// fits `keyof T`.
fn relate_subset_of_operand(
    actual: &Type,
    expected: &Type,
    types: TypeResolver,
    seen: &mut Vec<(Type, Type)>,
) -> bool {
    use super::utility_types::{Utility, deferred_application};
    matches!(
        deferred_application(actual),
        Some((
            Utility::Readonly | Utility::Required | Utility::Exclude | Utility::Extract,
            [operand, ..],
        )) if assignable_rec(operand, expected, types, seen)
    )
}

/// A form as the target: it holds only what the identities tsc proves for
/// every `T` put into it (`X` and `{}` fit `Partial<X>`), a key fits
/// `keyof T` when it is a key of `T`'s bound, and a value fits `T["a"]` when
/// the bound gives `a` that one value.
fn relate_into_form(
    actual: &Type,
    expected: &Type,
    types: TypeResolver,
    seen: &mut Vec<(Type, Type)>,
) -> bool {
    use super::utility_types::{Utility, deferred_application};
    if let Some((utility, args)) = deferred_application(expected) {
        return match (utility, args) {
            (Utility::Partial, _) if actual.is_empty_object() => true,
            (
                Utility::Partial | Utility::Readonly | Utility::Pick | Utility::Omit,
                [operand, ..],
            ) => operand == actual,
            _ => false,
        };
    }
    if let Type::KeyOf(operand) = expected
        && !actual.is_type_operator()
    {
        // A wider operand has fewer keys, so its upper bound's keys fit every
        // instantiation: `"b"` fits `keyof T["a"]` with `T extends { a: { b:
        // number } }`.
        let bound = concrete_upper_bound(operand, types);
        if matches!(bound.peel(), Type::Error) {
            return false;
        }
        return super::utility_types::keyof_type(&bound, types)
            .is_some_and(|keys| assignable_rec(actual, &keys, types, seen));
    }
    if let Type::IndexedAccess { object, index } = expected
        && !actual.is_type_operator()
    {
        // A property of `Partial<T>` may be absent: `patch[k] = undefined`.
        if matches!(actual.peel(), Type::Undefined)
            && matches!(deferred_application(object), Some((Utility::Partial, _)))
        {
            return true;
        }
        // A property the declared bound gives one value has it in every
        // instantiation (`T["length"]` of a pair is `2`), so `t.length`
        // fits. tsc accepts any value of the bound's property type here,
        // which a narrower `T` can contradict.
        return bound_properties(object, index, types).is_some_and(|properties| {
            properties.iter().all(|property| {
                let unit = property.peel();
                (unit.is_literal_type() || matches!(unit, Type::Null | Type::Undefined))
                    && assignable_rec(actual, unit, types, seen)
            })
        });
    }
    false
}

/// Whether `actual` may be written to the property `target` (`T[K]`)
/// names: tsc's rule once the index's bound names no type parameter. The
/// value must fit, for writing, the property each bound key names in the
/// declared bound (all of them, for a union of keys). So `t["a"] = 5`
/// writes `T["a"]` with `T extends { a: number }`, as `t.a = 5` does.
pub(super) fn fits_bound_property_write(actual: &Type, target: &Type, types: TypeResolver) -> bool {
    indexed_bound_properties(target, types).is_some_and(|properties| {
        properties
            .iter()
            .all(|property| assignable(actual, property, types))
    })
}

/// The type a write to `target` (`T[K]`) is checked against, as the value's
/// context: the declared bound's property when every bound key names the
/// same one, `None` when they differ, and `target` itself otherwise.
pub(super) fn write_context(target: &Type, types: TypeResolver) -> Option<Type> {
    let Some(properties) = indexed_bound_properties(target, types) else {
        return Some(target.clone());
    };
    match properties.split_first() {
        Some((first, rest)) if rest.iter().all(|property| property == first) => Some(first.clone()),
        _ => None,
    }
}

/// [`bound_properties`] of the form `target` (`T[K]`).
fn indexed_bound_properties(target: &Type, types: TypeResolver) -> Option<Vec<Type>> {
    // A write through `Pick<T, K>[K]` or `Readonly<T>[K]` writes `T[K]`,
    // as tsc rewrites it. `Required<T>[K]` keeps its own bound, whose
    // property holds no `undefined`.
    if let Some(property) = written_property(target) {
        return indexed_bound_properties(&property, types);
    }
    let Type::IndexedAccess { object, index } = target.without_aliases() else {
        return None;
    };
    bound_properties(object, index, types)
}

/// The properties `object[index]` names in its declared bounds, one per key
/// of the index's bound, which must all be literal. A type parameter's index
/// signature does not count: a narrower `T` may declare the key with a
/// narrower type. `None` when a bound is unknown.
fn bound_properties(object: &Type, index: &Type, types: TypeResolver) -> Option<Vec<Type>> {
    use super::utility_types::{index_access_type, members};
    let object_bound = declared_bound(object, types)?;
    let index_bound = concrete_upper_bound(index, types);
    if has_no_concrete_bound(&object_bound) || has_no_concrete_bound(&index_bound) {
        return None;
    }
    let keys = members(&index_bound);
    if keys.is_empty() || !keys.iter().all(Type::is_literal_type) {
        return None;
    }
    let object_bound = if object_bound == *object {
        object_bound
    } else {
        without_index_signature(object_bound, types)
    };
    keys.iter()
        .map(|key| index_access_type(&object_bound, key, types).ok())
        .collect()
}

/// Whether `ty` is still a type parameter or form, or has no bound at all.
fn has_no_concrete_bound(ty: &Type) -> bool {
    let open = ty.without_aliases();
    open.lowers_to_erased_slot() || matches!(open, Type::Error | Type::Unknown)
}

/// The bound tsc reads a property of `ty` through: a type parameter's
/// declared bound, as written, with the type parameters it names left in
/// place (`T extends { a: U }` gives `{ a: U }`). A form's base constraint
/// substitutes every bound, which is exact only when those bounds name no
/// other type parameter. Any other type is its own.
fn declared_bound(ty: &Type, types: TypeResolver) -> Option<Type> {
    let open = ty.without_aliases();
    if let Type::GenericParam { id, .. } = open {
        let mut bound = types.generic_constraints.get(id)?.clone();
        for _ in 0..crate::compiler_limits::MAX_TYPE_RESOLUTION_DEPTH {
            let Type::GenericParam { id, .. } = bound.without_aliases() else {
                return declared_bound(&bound, types);
            };
            bound = types.generic_constraints.get(id)?.clone();
        }
        return None;
    }
    if open.is_type_operator() {
        return closed_bounds(open, types).then(|| base_constraint(open, types));
    }
    Some(ty.clone())
}

/// Whether every type parameter in `ty` has a bound naming no other.
fn closed_bounds(ty: &Type, types: TypeResolver) -> bool {
    let mut closed = true;
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        if let Type::GenericParam { id, .. } = ty
            && types
                .generic_constraints
                .get(id)
                .is_some_and(super::utility_types::holds_type_parameter)
        {
            closed = false;
        }
        crate::type_size::for_each_child(ty, |child| pending.push(child));
    }
    closed
}

/// `ty` without its index signature, so only declared properties are read.
fn without_index_signature(ty: Type, types: TypeResolver) -> Type {
    match ty.peel() {
        Type::Object { fields, .. } => Type::Object {
            fields: fields.clone(),
            index: None,
            more_indexes: Vec::new(),
        },
        Type::InterfaceRef { .. } if types.has_index_signature(&ty) => {
            match expand_interface_data_shape(&ty, types) {
                Some(Type::Object { fields, .. }) => Type::Object {
                    fields,
                    index: None,
                    more_indexes: Vec::new(),
                },
                _ => ty,
            }
        }
        _ => ty,
    }
}

/// [`upper_bound`], followed through a type parameter it reaches: `T["a"]`
/// with `T extends { a: T }` is bounded by `T`, and so by `{ a: T }`. A
/// parameter's own bound is no parameter, so one more step suffices.
fn concrete_upper_bound(ty: &Type, types: TypeResolver) -> Type {
    let bound = upper_bound(ty, types, 0);
    if matches!(bound.without_aliases(), Type::GenericParam { .. }) && bound != *ty {
        return upper_bound(&bound, types, 0);
    }
    bound
}

/// A form as the source, by its base constraint ([`base_constraint`]).
fn relate_out_of_form(
    actual: &Type,
    expected: &Type,
    types: TypeResolver,
    seen: &mut Vec<(Type, Type)>,
) -> bool {
    let constraint = base_constraint(actual, types);
    // A constraint of `never` (`T[keyof T]` with `T` unbounded has no keys
    // to read) proves nothing about the values the form holds. Nor does one
    // that has no evaluation, which is `Error` and would fit anything.
    if constraint.is_type_operator() || matches!(constraint.peel(), Type::Never | Type::Error) {
        return false;
    }
    assignable_rec(&constraint, expected, types, seen)
}

/// tsc's identities for the keys of a mapped utility: `keyof Pick<T, K>` is
/// `K`, `keyof Omit<T, K>` is `Exclude<keyof T, K>`, and `Partial`,
/// `Readonly` and `Required` keep `T`'s keys. `None` for any other type.
fn mapped_keys(form: &Type) -> Option<Type> {
    use super::utility_types::{Utility, deferred, deferred_application};
    let Type::KeyOf(operand) = form.without_aliases() else {
        return None;
    };
    match deferred_application(operand)? {
        (Utility::Pick, [_, keys]) => Some(keys.clone()),
        (Utility::Omit, [object, keys]) => Some(deferred(
            Utility::Exclude,
            vec![Type::KeyOf(Box::new(object.clone())), keys.clone()],
        )),
        (Utility::Partial | Utility::Readonly | Utility::Required, [object]) => {
            Some(Type::KeyOf(Box::new(object.clone())))
        }
        _ => None,
    }
}

/// `M<T>[K]` as `T[K]` for a write, where `M` is `Pick` or `Readonly`.
fn written_property(form: &Type) -> Option<Type> {
    use super::utility_types::{Utility, deferred_application};
    let Type::IndexedAccess { object, index } = form.without_aliases() else {
        return None;
    };
    let mut object = object.as_ref();
    let mut mapped = false;
    while let Some((Utility::Readonly | Utility::Pick, [operand, ..])) =
        deferred_application(object)
    {
        object = operand;
        mapped = true;
    }
    mapped.then(|| Type::indexed_access(object.clone(), index.as_ref().clone()))
}

/// A property of a mapped utility, `M<T>[K]`, as `T[K]`.
enum MappedProperty {
    /// `Readonly`, `Required` (narrower) and `Pick` read `T`'s property.
    Same(Type),
    /// `Partial` makes it optional.
    Optional(Type),
}

/// `M<T>[K]` as `T[K]` for `M` a mapped utility; `None` for any other form.
fn mapped_property(form: &Type) -> Option<MappedProperty> {
    use super::utility_types::{Utility, deferred_application};
    let Type::IndexedAccess { object, index } = form.without_aliases() else {
        return None;
    };
    let mut object = object.as_ref();
    let mut mapped = false;
    loop {
        match deferred_application(object) {
            Some((Utility::Readonly | Utility::Required | Utility::Pick, [operand, ..])) => {
                object = operand;
                mapped = true;
            }
            Some((Utility::Partial, [operand])) => {
                return Some(MappedProperty::Optional(Type::indexed_access(
                    operand.clone(),
                    index.as_ref().clone(),
                )));
            }
            _ => break,
        }
    }
    mapped
        .then(|| MappedProperty::Same(Type::indexed_access(object.clone(), index.as_ref().clone())))
}

/// `NoInfer<X>` as `X`, and `NonNullable<X>` as the narrowing's `X & {}`, for
/// an application that still mentions a type parameter.
fn rewrite_to_operand_form(ty: &Type) -> Option<Type> {
    use super::utility_types::{Utility, deferred_application};
    match deferred_application(ty) {
        Some((Utility::NoInfer, [operand])) => Some(operand.clone()),
        Some((Utility::NonNullable, [operand])) => Some(Type::Refined {
            original: Box::new(operand.clone()),
            ty: Box::new(Type::empty_object()),
        }),
        _ => None,
    }
}

/// The type `ty` wraps in `Partial`, `Readonly` and `Required`, or `ty` itself.
fn homomorphic_base(ty: &Type) -> &Type {
    homomorphic_wrapping(ty).map_or(ty, |(base, _)| base)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Optionality {
    /// Properties as the base declares them.
    Unchanged,
    /// Every property optional (`Partial`).
    Optional,
    /// Every property required (`Required`).
    Required,
}

/// The type `ty` wraps in `Partial`, `Readonly` and `Required`, and the
/// optionality the outermost of them leaves. A type that is no such wrapping
/// is its own base, unchanged. `None` when that base holds no type
/// parameter, since only deferred wrappings need this.
fn homomorphic_wrapping(ty: &Type) -> Option<(&Type, Optionality)> {
    use super::utility_types::{Utility, deferred_application};
    let (base, optionality) = match deferred_application(ty) {
        Some((Utility::Partial, [operand])) => {
            (homomorphic_wrapping(operand)?.0, Optionality::Optional)
        }
        Some((Utility::Required, [operand])) => {
            (homomorphic_wrapping(operand)?.0, Optionality::Required)
        }
        Some((Utility::Readonly, [operand])) => homomorphic_wrapping(operand)?,
        Some(_) => return None,
        None => (ty, Optionality::Unchanged),
    };
    super::utility_types::holds_type_parameter(base).then_some((base, optionality))
}

/// Whether a signature `TypeVar` occurs anywhere in `ty`, refinements and
/// template holes included, which [`super::expr::type_contains_type_var`]
/// does not look into.
fn mentions_signature_var(ty: &Type) -> bool {
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        if matches!(ty, Type::TypeVar(_)) {
            return true;
        }
        crate::type_size::for_each_child(ty, |child| pending.push(child));
    }
    false
}

/// What every instantiation of the form `ty` is assignable to, tsc's base
/// constraint. A type parameter's bound stands in for it only where the form
/// grows with it; where it shrinks (`keyof T`, `Pick`'s keys, `Exclude`'s
/// filter, a parameter list) the form's own upper bound does. So
/// `T[keyof T]` with `T extends { a: number }` is `unknown`, not `number`.
pub(super) fn base_constraint(ty: &Type, types: TypeResolver) -> Type {
    use super::utility_types::{Utility, deferred_application, index_access_type};
    let upper = |operand: &Type| concrete_upper_bound(operand, types);
    let evaluate = |ty: Type| super::utility_types::normalize(&ty, types).unwrap_or(Type::Error);
    // `Pick<T, K>[K]` is `T[K]` (`Partial` adds `undefined`).
    if let Some(property) = mapped_property(ty) {
        return match property {
            MappedProperty::Same(property) => base_constraint(&property, types),
            MappedProperty::Optional(property) => {
                Type::union(vec![base_constraint(&property, types), Type::Undefined])
            }
        };
    }
    match ty.without_aliases() {
        // A narrower `T` has more keys: only every key bounds `keyof T`.
        Type::KeyOf(_) => Type::union(vec![Type::String, Type::Number]),
        Type::IndexedAccess { object, index } => {
            index_access_type(&upper(object), &upper(index), types).unwrap_or(Type::Unknown)
        }
        operator => match deferred_application(operator) {
            // A member `Exclude` keeps lies under a bound member it keeps.
            Some((Utility::Exclude, [operand, filter])) => {
                let operand = upper(operand);
                if super::utility_types::holds_type_parameter(filter) {
                    return operand;
                }
                evaluate(super::utility_types::deferred(
                    Utility::Exclude,
                    vec![operand, filter.clone()],
                ))
            }
            // Each bound member that fits the filter, else the filter's
            // members that fit the bound member (tsc's member & filter): with
            // `T extends string | number`, `Extract<T, "a" | number>` is
            // bounded by `"a" | number`.
            Some((Utility::Extract, [operand, filter])) => {
                let operand = upper(operand);
                if matches!(operand.peel(), Type::Unknown)
                    || super::utility_types::holds_type_parameter(filter)
                {
                    return upper(filter);
                }
                let filter_members = super::utility_types::members(filter);
                let bounded: Vec<Type> = super::utility_types::members(&operand)
                    .into_iter()
                    .flat_map(|member| {
                        if assignable(&member, filter, types) {
                            vec![member]
                        } else {
                            filter_members
                                .iter()
                                .filter(|part| assignable(part, &member, types))
                                .cloned()
                                .collect()
                        }
                    })
                    .collect();
                // No bound member meets the filter: the filter still bounds
                // what `Extract` keeps.
                if bounded.is_empty() {
                    return upper(filter);
                }
                Type::union(bounded)
            }
            // `Pick<X, keyof X>` is `X`'s homomorphic copy. Any other `Pick`
            // holds the keys every instantiation of its key type includes,
            // as tsc reads it: none for a bare `K`, and `"b"` for
            // `Extract<keyof T, "b">` with `T extends { a; b }`.
            Some((Utility::Pick, [object, Type::KeyOf(keys_of)])) if keys_of.as_ref() == object => {
                upper(object)
            }
            Some((Utility::Pick, [object, keys])) => evaluate(super::utility_types::deferred(
                Utility::Pick,
                vec![upper(object), lower_keys(keys, types)],
            )),
            // The operand's bound without `null` and `undefined`: with `T
            // extends { next: T | null }`, `NonNullable<T["next"]>` is
            // bounded by `T`, so by its bound.
            Some((Utility::NonNullable, [operand])) => {
                let kept = Type::union(
                    super::utility_types::members(&upper(operand))
                        .into_iter()
                        .filter(|member| !matches!(member.peel(), Type::Null | Type::Undefined))
                        .collect(),
                );
                upper(&kept)
            }
            // A narrower function may take more parameters.
            Some((Utility::Parameters, _)) => Type::Unknown,
            Some((utility, args)) => evaluate(super::utility_types::deferred(
                utility,
                args.iter().map(&upper).collect(),
            )),
            // Not a form: callers pass only forms.
            None => substitute_bounds(operator, types),
        },
    }
}

/// The keys every instantiation of the key type `keys` includes: `keyof T`
/// holds at least its bound's keys, `Extract` and `Exclude` keep what they
/// keep of those, and a bare type parameter may be `never`.
fn lower_keys(keys: &Type, types: TypeResolver) -> Type {
    use super::utility_types::{
        Utility, deferred, deferred_application, holds_type_parameter, keyof_type, normalize,
    };
    // A lower bound that has no evaluation guarantees no key.
    let evaluate = |ty: Type| normalize(&ty, types).unwrap_or(Type::Never);
    let open = keys.without_aliases();
    if !holds_type_parameter(open) {
        return keys.clone();
    }
    match open {
        Type::KeyOf(operand) => {
            keyof_type(&concrete_upper_bound(operand, types), types).unwrap_or(Type::Never)
        }
        Type::Union(members) => Type::union(
            members
                .iter()
                .map(|member| lower_keys(member, types))
                .collect(),
        ),
        other => match deferred_application(other) {
            Some((Utility::Extract, [operand, filter])) if !holds_type_parameter(filter) => {
                evaluate(deferred(
                    Utility::Extract,
                    vec![lower_keys(operand, types), filter.clone()],
                ))
            }
            Some((Utility::Exclude, [operand, filter])) => evaluate(deferred(
                Utility::Exclude,
                vec![
                    lower_keys(operand, types),
                    concrete_upper_bound(filter, types),
                ],
            )),
            _ => Type::Never,
        },
    }
}

/// What every instantiation of `ty` is assignable to: a form's base
/// constraint, a type parameter's bound followed through bounds that are
/// themselves forms (`K extends keyof T` is bounded by every key), and any
/// other type with its type parameters replaced by their bounds.
fn upper_bound(ty: &Type, types: TypeResolver, depth: u32) -> Type {
    if depth > crate::compiler_limits::MAX_TYPE_RESOLUTION_DEPTH {
        return Type::Error;
    }
    if ty.without_aliases().is_type_operator() {
        return base_constraint(ty, types);
    }
    if let Type::GenericParam { id, .. } = ty.without_aliases()
        && let Some(bound) = types.generic_constraints.get(id)
        && (bound.without_aliases().is_type_operator()
            || matches!(bound.without_aliases(), Type::GenericParam { .. }))
    {
        return upper_bound(bound, types, depth + 1);
    }
    substitute_bounds(ty, types)
}

/// `ty` with each body type parameter replaced by its bound (`unknown` when it
/// has none). An upper bound only where `ty` grows with its parameters; for
/// forms such as `keyof T`, [`upper_bound`] is. A parameter met again inside
/// its own bound (`T extends { left: T | null }`) stays itself, as the bound
/// states it, so each bound is substituted once per path.
///
/// Bounds that name each other (`P0 extends { x: P1; y: P1 }`, ...) copy
/// each bound per path, so the result builds under the type budget and is
/// `Error` past it.
pub(super) fn substitute_bounds(ty: &Type, types: TypeResolver) -> Type {
    fn replace(
        ty: &Type,
        types: TypeResolver,
        open: &mut Vec<u32>,
        budget: &mut crate::type_size::TypeBudget<'_>,
        depth: u32,
    ) -> Result<Type, crate::type_size::TypeTooLarge> {
        budget.charge(depth)?;
        match ty {
            Type::GenericParam { id, .. } if open.contains(id) => Ok(ty.clone()),
            Type::GenericParam { id, .. } => match types.generic_constraints.get(id) {
                Some(bound) => {
                    open.push(*id);
                    let replaced = replace(bound, types, open, budget, depth);
                    open.pop();
                    replaced
                }
                None => Ok(Type::Unknown),
            },
            _ => crate::type_size::map_children(ty, |child| {
                replace(child, types, open, budget, depth.saturating_add(1))
            }),
        }
    }
    let mut budget = types.limits.budget();
    match replace(ty, types, &mut Vec::new(), &mut budget, 1) {
        Ok(ty) => ty,
        Err(exceeded) => {
            types.limits.record(exceeded);
            Type::Error
        }
    }
}

/// Whether `actual` fits a symbolic string mapping such as `Uppercase<string>`:
/// only strings the mapping leaves unchanged do, so `"ABC"` fits and `"abc"`
/// and `string` do not.
fn fits_symbolic_mapping(chain: &[super::utility_types::Utility], actual: &Type) -> bool {
    use super::utility_types::{Utility, chain_preserves, symbolic_chain};
    // The mappings are idempotent, so whatever `m` produces is a fixed point
    // of `m`: `Uppercase<Lowercase<string>>` fits `Uppercase<string>`.
    if let Some(actual_chain) = symbolic_chain(actual) {
        return actual_chain == chain || (chain.len() == 1 && actual_chain.last() == chain.last());
    }
    match actual.without_aliases() {
        Type::Never | Type::Error => true,
        Type::StringLiteral(text) => chain_preserves(chain, text),
        Type::StringEnum {
            member: Some(member),
            ..
        } => chain_preserves(chain, &member.value),
        Type::TemplateLiteral {
            parts,
            substitutions,
        } => {
            if let [mapping @ (Utility::Capitalize | Utility::Uncapitalize)] = chain {
                // Only the first character is mapped.
                return match parts.first() {
                    Some(head) if !head.is_empty() => chain_preserves(&[*mapping], head),
                    _ => substitutions
                        .first()
                        .is_none_or(|hole| fits_symbolic_mapping(chain, hole)),
                };
            }
            parts.iter().all(|part| chain_preserves(chain, part))
                && substitutions
                    .iter()
                    .all(|hole| fits_symbolic_mapping(chain, hole))
        }
        _ => false,
    }
}

/// Whether `ty` is an interface whose values only the host builds, such as
/// `fs.Stat`. Its properties read the host's backing struct, so an object or
/// class of the same shape can't stand in for one: it is nominal. As a target
/// it refuses every structural source, in the `(Object, InterfaceRef)` and
/// `(ClassRef, InterfaceRef)` arms of `assignable_rec` and in
/// `satisfies_structurally`.
pub(crate) fn is_host_backed_interface(ty: &Type) -> bool {
    let Type::InterfaceRef { mangled, .. } = ty.peel() else {
        return false;
    };
    crate::stdlib::host_views().any(|view| {
        mangled.as_str().split_once(crate::mangle::SEP) == Some((view.module, view.interface))
    })
}

/// The prelude wrappers of a primitive (`Number`, `Boolean`, `String`,
/// `BigInt`). A value of one holds either the primitive or a structural object.
pub(crate) fn is_wrapper_interface(ty: &Type) -> bool {
    let Type::InterfaceRef { mangled, .. } = ty.peel() else {
        return false;
    };
    wrapper_interface_name(mangled).is_some()
}

/// The prelude name of `mangled` when it is a primitive wrapper interface.
pub(crate) fn wrapper_interface_name(mangled: &crate::MangledName) -> Option<&'static str> {
    ["Number", "Boolean", "String", "BigInt"]
        .into_iter()
        .find(|name| *mangled == crate::mangle::prelude(name))
}

/// `null` and `undefined` are excluded: they narrow through `predicate_envs_eq_nullish`.
pub(super) fn literal_value_of(kind: &crate::TypedExprKind) -> Option<narrowing::LiteralValue> {
    use crate::TypedExprKind;
    match kind {
        TypedExprKind::Number(n) => Some(narrowing::LiteralValue::Number(
            crate::types::LiteralF64(*n),
        )),
        TypedExprKind::String(s) => Some(narrowing::LiteralValue::String(s.clone())),
        TypedExprKind::Boolean(b) => Some(narrowing::LiteralValue::Boolean(*b)),
        TypedExprKind::BigInt(digits) => match crate::types::bigint_literal_type(digits) {
            Type::BigIntLiteral(canonical) => Some(narrowing::LiteralValue::BigInt(canonical)),
            _ => None,
        },
        _ => None,
    }
}

pub(super) fn literal_to_type(lit: &narrowing::LiteralValue) -> Type {
    match lit {
        narrowing::LiteralValue::Number(n) => Type::NumberLiteral(*n),
        narrowing::LiteralValue::String(s) => Type::StringLiteral(s.clone()),
        narrowing::LiteralValue::Boolean(b) => Type::BooleanLiteral(*b),
        narrowing::LiteralValue::BigInt(digits) => Type::BigIntLiteral(digits.clone()),
        narrowing::LiteralValue::Null => Type::Null,
        narrowing::LiteralValue::Undefined => Type::Undefined,
    }
}

/// Whether `actual`, a refinement, fits the union
/// `expected` (through aliases only: an expected refinement keeps requiring
/// its own original). It fits through one member that neither its original
/// nor its shape alone fits (`T & {}` into `(T & {}) | undefined`), or piece by
/// piece: `T & ({} | null)` is `(T & {}) | null`. When it doesn't, the
/// refined-expected and refined-actual rules still get their turn.
fn refinement_fits_union(
    actual: &Type,
    expected: &Type,
    types: TypeResolver,
    seen: &mut Vec<(Type, Type)>,
) -> bool {
    let Type::Union(members) = expected.without_aliases() else {
        return false;
    };
    if members
        .iter()
        .any(|member| assignable_rec(actual, member, types, seen))
    {
        return true;
    }
    let Some(pieces) = actual.refinement_pieces() else {
        return false;
    };
    pieces
        .iter()
        .all(|piece| assignable_rec(piece, expected, types, seen))
}

/// A hybrid's parts: an intersection with a function member (as
/// `Object.assign(fn, props)` makes) is that function, the intersection of its
/// function members, and the properties of its object members, an earlier
/// member's winning.
pub(super) fn hybrid_parts(ty: &Type) -> Option<(Type, BTreeMap<String, ObjectField>)> {
    let Type::Intersection(members) = ty.peel() else {
        return None;
    };
    let (functions, rest): (Vec<&Type>, Vec<&Type>) = members
        .iter()
        .partition(|member| matches!(member.peel(), Type::Function { .. }));
    if functions.is_empty() {
        return None;
    }
    let mut fields = BTreeMap::new();
    for member in rest {
        if let Type::Object { fields: own, .. } = member.peel() {
            for (name, field) in own {
                fields.entry(name.clone()).or_insert_with(|| field.clone());
            }
        }
    }
    Some((
        Type::intersection(functions.into_iter().cloned().collect()),
        fields,
    ))
}

/// Whether `ty` is a hybrid; see [`hybrid_parts`].
pub(super) fn is_hybrid(ty: &Type) -> bool {
    hybrid_parts(ty).is_some()
}

/// Whether a hybrid satisfies an interface with a call signature: its function
/// fits the call signature, and its properties the interface's named members.
/// Neither alone does.
fn hybrid_satisfies_interface(
    actual: &Type,
    expected: &Type,
    types: TypeResolver,
    seen: &mut Vec<(Type, Type)>,
) -> bool {
    if is_host_backed_interface(expected)
        || types.has_index_signature(expected)
        || types.signature_of(expected, Signature::Construct).is_some()
    {
        return false;
    }
    let Some(call) = types.signature_of(expected, Signature::Call) else {
        return false;
    };
    let Some((mangled, _, name, args)) = expected.interface_routing() else {
        return false;
    };
    let Some(form) = types.interface_full_form(&mangled, name, &args) else {
        return false;
    };
    let Some((function, fields)) = hybrid_parts(actual) else {
        return false;
    };
    assignable_rec(&function, &call, types, seen)
        && form
            .iter()
            .all(|(member, expected)| match fields.get(member) {
                Some(actual) => {
                    (expected.optional || !actual.optional)
                        && assignable_rec(&actual.read_ty(), &expected.read_ty(), types, seen)
                }
                None => expected.optional,
            })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2-arg shim so the existing tests read unchanged. None exercise
    /// recursive aliases, so empty tables are sufficient; it shadows the
    /// glob-imported 3-arg `assignable` for unqualified calls in this module.
    fn assignable(actual: &Type, expected: &Type) -> bool {
        let types = TypeNamespace::new();
        let registry = TypeRegistry::new();
        let limits = crate::type_size::TypeLimits::default();
        super::assignable(
            actual,
            expected,
            TypeResolver {
                types: &types,
                registry: &registry,
                limits: &limits,
                strict_subtype: false,
                generic_constraints: &BTreeMap::new(),
            },
        )
    }

    fn fn_ty(params: Vec<Type>, ret: Type) -> Type {
        Type::Function {
            params,
            ret: Box::new(ret),
            predicate: None,
            has_rest: false,
            optional: 0,
        }
    }

    fn predicate_fn_ty(kind: crate::PredicateKind) -> Type {
        Type::Function {
            params: vec![Type::Unknown],
            ret: Box::new(kind.return_type()),
            predicate: Some(Box::new(crate::TypePredicate {
                parameter_index: 0,
                asserted_type: Type::String,
                kind,
            })),
            has_rest: false,
            optional: 0,
        }
    }

    #[test]
    fn assertion_functions_relate_like_tsc() {
        use crate::PredicateKind::{Asserts, Guard};
        let asserts = predicate_fn_ty(Asserts);
        let guard = predicate_fn_ty(Guard);
        assert!(assignable(&asserts, &asserts));
        assert!(!assignable(&asserts, &guard));
        assert!(!assignable(&guard, &asserts));
        assert!(assignable(
            &asserts,
            &fn_ty(vec![Type::Unknown], Type::Void)
        ));
        assert!(!assignable(
            &asserts,
            &fn_ty(vec![Type::Unknown], Type::Boolean)
        ));
        assert!(!assignable(
            &fn_ty(vec![Type::Unknown], Type::Void),
            &asserts
        ));
    }

    #[test]
    fn undefined_is_distinct_from_null_and_assignable_to_void() {
        assert!(assignable(&Type::Undefined, &Type::Void));
        assert!(assignable(&Type::Void, &Type::Unknown));
        assert!(!assignable(&Type::Void, &Type::Undefined));
        assert!(!assignable(&Type::Null, &Type::Undefined));
        assert!(!assignable(&Type::Undefined, &Type::Null));
    }

    #[test]
    fn optional_tuple_accepts_omission_but_cannot_satisfy_required_position() {
        let optional = Type::Tuple(crate::types::TupleType {
            elements: vec![
                Type::Number,
                Type::union(vec![Type::String, Type::Undefined]),
            ],
            optional: 1,
        });
        let short = Type::Tuple(vec![Type::Number].into());
        let full = Type::Tuple(vec![Type::Number, Type::String].into());
        assert!(assignable(&short, &optional));
        assert!(assignable(&full, &optional));
        assert!(!assignable(&optional, &short));
        assert!(!assignable(&optional, &full));
    }

    #[test]
    fn optional_function_parameter_can_be_omitted_by_target_callers() {
        let optional = Type::Function {
            params: vec![Type::union(vec![Type::String, Type::Undefined])],
            ret: Box::new(Type::Void),
            predicate: None,
            has_rest: false,
            optional: 1,
        };
        let no_args = fn_ty(vec![], Type::Void);
        let required = fn_ty(vec![Type::String], Type::Void);
        assert!(assignable(&optional, &no_args));
        assert!(assignable(&optional, &required));
        assert!(!assignable(&required, &optional));
        let required_union = fn_ty(
            vec![Type::union(vec![Type::String, Type::Undefined])],
            Type::Void,
        );
        assert!(assignable(&required_union, &optional));
        assert!(!assignable(&required_union, &no_args));
    }

    #[test]
    fn optional_object_field_accepts_explicit_undefined() {
        let object = |field| Type::Object {
            more_indexes: Vec::new(),
            fields: [("value".to_string(), field)].into_iter().collect(),
            index: None,
        };
        let optional = object(ObjectField::optional(Type::String));
        assert!(assignable(
            &object(ObjectField::required(Type::Undefined)),
            &optional
        ));
        assert!(!assignable(
            &object(ObjectField::required(Type::Null)),
            &optional
        ));
    }

    #[test]
    fn assignable_function_exact_match() {
        let a = fn_ty(vec![Type::Number], Type::String);
        let b = fn_ty(vec![Type::Number], Type::String);
        assert!(assignable(&a, &b));
        assert!(assignable(&b, &a));
    }

    #[test]
    fn assignable_function_may_take_fewer_params_but_not_more() {
        let one = fn_ty(vec![Type::Number], Type::Void);
        let two = fn_ty(vec![Type::Number, Type::Number], Type::Void);
        assert!(assignable(&one, &two));
        assert!(!assignable(&two, &one));
    }

    #[test]
    fn assignable_function_return_mismatch() {
        let a = fn_ty(vec![Type::Number], Type::Number);
        let b = fn_ty(vec![Type::Number], Type::String);
        assert!(!assignable(&a, &b));
        assert!(!assignable(&b, &a));
    }

    #[test]
    fn assignable_function_param_mismatch() {
        let a = fn_ty(vec![Type::Number], Type::Void);
        let b = fn_ty(vec![Type::String], Type::Void);
        assert!(!assignable(&a, &b));
        assert!(!assignable(&b, &a));
    }

    #[test]
    fn assignable_function_error_param_is_bidirectional() {
        // Cascading diagnostics don't pile up when a param failed to resolve.
        let a = fn_ty(vec![Type::Error], Type::Void);
        let b = fn_ty(vec![Type::Number], Type::Void);
        assert!(assignable(&a, &b));
        assert!(assignable(&b, &a));
    }

    #[test]
    fn assignable_function_error_return_is_bidirectional() {
        let a = fn_ty(vec![Type::Number], Type::Error);
        let b = fn_ty(vec![Type::Number], Type::String);
        assert!(assignable(&a, &b));
        assert!(assignable(&b, &a));
    }

    #[test]
    fn assignable_function_param_contravariance_uses_assignable() {
        let actual = fn_ty(vec![Type::Number], Type::Void);
        let expected = fn_ty(vec![Type::Error], Type::Void);
        assert!(assignable(&actual, &expected));
        assert!(assignable(&expected, &actual));
    }

    #[test]
    fn assignable_function_return_covariance_uses_assignable() {
        let actual = fn_ty(vec![Type::Number], Type::Error);
        let expected = fn_ty(vec![Type::Number], Type::String);
        assert!(assignable(&actual, &expected));
        assert!(assignable(&expected, &actual));
    }

    #[test]
    fn assignable_nested_function_recurses() {
        let inner = fn_ty(vec![Type::Number], Type::Number);
        let outer = fn_ty(vec![Type::Number], inner.clone());
        assert!(assignable(&outer, &outer));
        let inner_bad = fn_ty(vec![Type::Number], Type::String);
        let outer_bad = fn_ty(vec![Type::Number], inner_bad);
        assert!(!assignable(&outer, &outer_bad));
        assert!(!assignable(&outer_bad, &outer));
    }

    #[test]
    fn assignable_zero_arity_function() {
        let a = fn_ty(vec![], Type::Number);
        let b = fn_ty(vec![], Type::Number);
        assert!(assignable(&a, &b));
    }

    #[test]
    fn assignable_function_vs_non_function_rejected() {
        let a = fn_ty(vec![Type::Number], Type::Void);
        let b = Type::Number;
        assert!(!assignable(&a, &b));
        assert!(!assignable(&b, &a));
    }

    #[test]
    fn assignable_non_function_still_structural_eq() {
        assert!(assignable(&Type::Number, &Type::Number));
        assert!(!assignable(&Type::Number, &Type::String));
        assert!(assignable(&Type::Error, &Type::String));
        assert!(assignable(&Type::String, &Type::Error));
    }

    #[test]
    fn assignable_member_to_union() {
        let u = Type::union(vec![Type::Number, Type::String]);
        assert!(assignable(&Type::Number, &u));
        assert!(assignable(&Type::String, &u));
    }

    #[test]
    fn assignable_non_member_to_union_fails() {
        let u = Type::union(vec![Type::Number, Type::String]);
        assert!(!assignable(&Type::Boolean, &u));
    }

    #[test]
    fn assignable_union_to_superset_union() {
        let actual = Type::union(vec![Type::Number, Type::String]);
        let expected = Type::union(vec![Type::Number, Type::String, Type::Boolean]);
        assert!(assignable(&actual, &expected));
    }

    #[test]
    fn assignable_union_to_subset_union_fails() {
        let actual = Type::union(vec![Type::Number, Type::String]);
        let expected = Type::union(vec![Type::Number, Type::Boolean]);
        assert!(!assignable(&actual, &expected));
    }

    #[test]
    fn assignable_union_to_member_fails() {
        let actual = Type::union(vec![Type::Number, Type::String]);
        assert!(!assignable(&actual, &Type::Number));
    }

    #[test]
    fn assignable_union_with_error_member_is_bidirectional() {
        let with_error = Type::union(vec![Type::Number, Type::Error]);
        assert_eq!(with_error, Type::Error);
        assert!(assignable(&with_error, &Type::Boolean));
        assert!(assignable(&Type::Boolean, &with_error));
    }

    #[test]
    fn assignable_array_of_union_recurses() {
        let union_elem = Type::union(vec![Type::Number, Type::String]);
        let union_arr = Type::Array(Box::new(union_elem));
        assert!(assignable(&union_arr, &union_arr));
        let narrow_arr = Type::Array(Box::new(Type::Number));
        assert!(assignable(&narrow_arr, &union_arr));
        assert!(!assignable(&union_arr, &narrow_arr));
    }

    #[test]
    fn readonly_accepts_mutable_but_not_the_reverse() {
        let array = |elem: Type| Type::Array(Box::new(elem));
        let readonly = |ty: Type| Type::Readonly(Box::new(ty));
        let numbers = array(Type::Number);
        let either = array(Type::union(vec![Type::Number, Type::String]));
        assert!(assignable(&numbers, &readonly(numbers.clone())));
        assert!(!assignable(&readonly(numbers.clone()), &numbers));
        // Covariant in the element, like the array it wraps.
        assert!(assignable(
            &readonly(numbers.clone()),
            &readonly(either.clone())
        ));
        assert!(!assignable(&readonly(either), &readonly(numbers.clone())));
        let pair = Type::Tuple(vec![Type::Number, Type::Number].into());
        assert!(assignable(&pair, &readonly(pair.clone())));
        assert!(!assignable(&readonly(pair.clone()), &pair));
        assert!(assignable(
            &readonly(pair.clone()),
            &readonly(numbers.clone())
        ));
        assert!(!assignable(&readonly(pair), &numbers));
        assert!(assignable(&readonly(numbers), &Type::Unknown));
    }

    #[test]
    fn assignable_anything_to_unknown() {
        for t in [
            Type::Number,
            Type::String,
            Type::Boolean,
            Type::Null,
            Type::Array(Box::new(Type::Number)),
            Type::Object {
                more_indexes: Vec::new(),
                index: None,
                fields: std::collections::BTreeMap::new(),
            },
            Type::union(vec![Type::Number, Type::String]),
        ] {
            assert!(
                assignable(&t, &Type::Unknown),
                "{t} should be assignable to unknown",
            );
        }
    }

    #[test]
    fn assignable_unknown_to_concrete_fails() {
        for t in [
            Type::Number,
            Type::String,
            Type::Boolean,
            Type::Null,
            Type::Array(Box::new(Type::Number)),
        ] {
            assert!(
                !assignable(&Type::Unknown, &t),
                "unknown should NOT flow into {t}",
            );
        }
    }

    #[test]
    fn assignable_unknown_to_unknown_identity() {
        assert!(assignable(&Type::Unknown, &Type::Unknown));
    }

    #[test]
    fn union_with_unknown_collapses() {
        assert_eq!(
            Type::union(vec![Type::Number, Type::Unknown]),
            Type::Unknown,
        );
        assert_eq!(
            Type::union(vec![Type::Unknown, Type::String, Type::Null]),
            Type::Unknown,
        );
    }

    fn obj(fields: Vec<(&str, Type, bool)>) -> Type {
        Type::Object {
            more_indexes: Vec::new(),
            index: None,
            fields: fields
                .into_iter()
                .map(|(k, ty, optional)| {
                    (
                        k.to_string(),
                        crate::ObjectField {
                            ty,
                            optional,
                            readonly: false,
                            method: false,
                        },
                    )
                })
                .collect(),
        }
    }

    fn iref(name: &str) -> Type {
        Type::interface_ref(
            crate::types::Package::user(),
            name,
            crate::mangle::package_symbol(crate::mangle::USER_PACKAGE, name),
            Vec::new(),
        )
    }

    /// A namespace holding a single interface named `name` with the given
    /// properties; `has_method` adds a method so the interface is no longer
    /// data-only.
    fn ns_with_interface(
        name: &'static str,
        properties: Vec<(&'static str, Type, bool)>,
        has_method: bool,
    ) -> TypeNamespace<'static> {
        use crate::package_declaration::{Dispatch, MethodSig, PropertySig};
        use std::collections::BTreeMap;
        let properties: BTreeMap<String, PropertySig> = properties
            .into_iter()
            .map(|(field, ty, optional)| {
                (
                    field.to_string(),
                    PropertySig {
                        ty,
                        readonly: false,
                        intrinsic: false,
                        optional,
                        doc: None,
                    },
                )
            })
            .collect();
        let mut methods = BTreeMap::new();
        if has_method {
            methods.insert(
                "m".to_string(),
                MethodSig {
                    overloads: Vec::new(),
                    optional: false,
                    generics: Default::default(),
                    params: Vec::new(),
                    ret: Type::Void,
                    predicate: None,
                    doc: None,
                },
            );
        }
        let mut ns = TypeNamespace::new();
        ns.insert(
            name.to_string(),
            crate::mangle::USER_PACKAGE.to_string(),
            crate::TypeSymbol {
                name: name.to_string(),
                mangled_name: crate::mangle::package_symbol(crate::mangle::USER_PACKAGE, name),
                declaration_span: crate::Span::at(crate::FileId(0)),
                kind: TypeKind::Interface {
                    extends: Vec::new(),
                    more_indexes: Vec::new(),
                    index: None,
                    generics: Default::default(),
                    methods,
                    properties,
                    call: None,
                    construct: None,
                    dispatch: Dispatch::VTable,
                    doc: None,
                },
            },
        );
        ns
    }

    fn assignable_with(actual: &Type, expected: &Type, ns: &TypeNamespace) -> bool {
        let registry = TypeRegistry::new();
        let limits = crate::type_size::TypeLimits::default();
        super::assignable(
            actual,
            expected,
            TypeResolver {
                types: ns,
                registry: &registry,
                limits: &limits,
                strict_subtype: false,
                generic_constraints: &BTreeMap::new(),
            },
        )
    }

    #[test]
    fn object_value_assignable_to_data_only_interface_both_ways() {
        let ns = ns_with_interface("Issue", vec![("id", Type::String, false)], false);
        let object = obj(vec![("id", Type::String, false)]);
        let iface = iref("Issue");
        assert!(assignable_with(&object, &iface, &ns), "object -> interface");
        assert!(assignable_with(&iface, &object, &ns), "interface -> object");
    }

    #[test]
    fn object_width_subtyping_into_interface() {
        // Extra fields on the object are fine when assigning into the interface.
        let ns = ns_with_interface("Issue", vec![("id", Type::String, false)], false);
        let wider = obj(vec![
            ("id", Type::String, false),
            ("extra", Type::Number, false),
        ]);
        assert!(assignable_with(&wider, &iref("Issue"), &ns));
    }

    #[test]
    fn object_missing_required_interface_field_rejected() {
        let ns = ns_with_interface(
            "Issue",
            vec![("id", Type::String, false), ("title", Type::String, false)],
            false,
        );
        let only_id = obj(vec![("id", Type::String, false)]);
        assert!(!assignable_with(&only_id, &iref("Issue"), &ns));
    }

    #[test]
    fn method_bearing_interface_not_structurally_assignable() {
        let ns = ns_with_interface("Logger", vec![("id", Type::String, false)], true);
        let object = obj(vec![("id", Type::String, false)]);
        assert!(
            !assignable_with(&object, &iref("Logger"), &ns),
            "object -> method iface"
        );
        assert!(
            !assignable_with(&iref("Logger"), &object, &ns),
            "method iface -> object"
        );
    }
}
