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
    pub(super) types: &'a TypeNamespace<'a>,
    pub(super) registry: &'a TypeRegistry<'a>,
    /// Records an oversized type met while resolving, for the inferer's next
    /// checkpoint; the resolver itself answers as if the type were `Error`.
    pub(crate) limits: &'a crate::type_size::TypeLimits,
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
            if !sig.generics.is_empty() {
                continue;
            }
            out.insert(
                member.clone(),
                ObjectField {
                    ty: Type::Function {
                        params: sig
                            .params
                            .iter()
                            .map(|p| substitute_or_record(&p.ty, &bindings, self.limits))
                            .collect(),
                        ret: Box::new(substitute_or_record(&sig.ret, &bindings, self.limits)),
                        predicate: None,
                        has_rest: sig.params.last().is_some_and(|p| p.rest),
                    },
                    optional: false,
                    readonly: true,
                    method: true,
                },
            );
        }
        for (member, sig) in properties {
            out.insert(
                member.clone(),
                ObjectField {
                    ty: substitute_or_record(&sig.ty, &bindings, self.limits),
                    optional: sig.optional,
                    readonly: sig.readonly,
                    method: false,
                },
            );
        }
        Some(out)
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
                    if !sig.generics.is_empty()
                        || method_visibility.get(name).copied() == Some(crate::Visibility::Private)
                    {
                        continue;
                    }
                    out.entry(name.clone()).or_insert(ObjectField {
                        ty: self.method_type(sig, bindings),
                        optional: false,
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
    fn method_type(&self, sig: &MethodSig, bindings: &BTreeMap<String, Type>) -> Type {
        Type::Function {
            params: sig
                .params
                .iter()
                .map(|p| substitute_or_record(&p.ty, bindings, self.limits))
                .collect(),
            ret: Box::new(substitute_or_record(&sig.ret, bindings, self.limits)),
            predicate: None,
            has_rest: sig.params.last().is_some_and(|p| p.rest),
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
        let (Some(class_form), Some(iface_form)) = (
            self.class_full_form(class_mangled, class_args),
            self.interface_full_form(iface_mangled, iface_name, iface_args),
        ) else {
            return Vec::new();
        };
        let mut failures = Vec::new();
        for (member, exp) in &iface_form {
            match class_form.get(member) {
                Some(act) => {
                    let mut seen = Vec::new();
                    // A setter's parameter is not a readable value. Check for a
                    // getter before comparing types, then report mutability or
                    // optionality only when the readable type is compatible.
                    //
                    // `readonly` is shallow, so the type check stays covariant. But a
                    // writable interface member can be written through the interface
                    // reference, so a `readonly`/get-only class member can't satisfy
                    // it. Methods are modelled as `readonly`, so they're unaffected.
                    if self.class_property_is_write_only(class_mangled, class_args, member) {
                        failures.push(ImplementsFailure::NotReadable {
                            member: member.clone(),
                            ty: exp.ty.to_string(),
                        });
                    } else if !self.class_member_assignable(
                        class_mangled,
                        class_args,
                        member,
                        &act.ty,
                        &exp.ty,
                        &mut seen,
                    ) {
                        failures.push(ImplementsFailure::Incompatible {
                            member: member.clone(),
                            expected: exp.ty.to_string(),
                            actual: act.ty.to_string(),
                        });
                    } else if !exp.readonly && act.readonly {
                        failures.push(ImplementsFailure::NotWritable {
                            member: member.clone(),
                            ty: exp.ty.to_string(),
                        });
                    } else if !exp.optional && act.optional {
                        failures.push(ImplementsFailure::OptionalityMismatch {
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
                    params: params[..expected_params.len()].to_vec(),
                    ret: ret.clone(),
                    predicate: predicate.clone(),
                    has_rest: false,
                },
                expected,
                self,
                seen,
            )
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
        let TypeKind::Interface {
            generics,
            properties,
            methods,
            ..
        } = &self.lookup(mangled, name)?.kind
        else {
            return None;
        };
        if !methods.is_empty() {
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

    /// Whether `name` resolves to an interface whose values are inert
    /// `Dispatch::Static` receivers (`console`, `Number`): typed nulls with no
    /// vtable behind them.
    pub(super) fn is_static_interface(&self, mangled: &MangledName, name: &str) -> bool {
        matches!(
            self.lookup(mangled, name).map(|symbol| &symbol.kind),
            Some(TypeKind::Interface {
                dispatch: crate::Dispatch::Static,
                ..
            })
        )
    }

    /// True when `name` resolves to an interface that declares methods — the
    /// nominal-only interfaces `interface_data_shape` refuses to expand.
    pub(super) fn interface_has_methods(&self, mangled: &MangledName, name: &str) -> bool {
        match self.lookup(mangled, name).map(|symbol| &symbol.kind) {
            Some(TypeKind::Interface { methods, .. }) => !methods.is_empty(),
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

pub(crate) fn assignable(actual: &Type, expected: &Type, types: TypeResolver) -> bool {
    // Coinductive assumption set for recursive-alias (`AliasRef`)
    // expansion: a pair re-encountered mid-proof is assumed to hold, so
    // comparing recursive types terminates.
    let mut seen: Vec<(Type, Type)> = Vec::new();
    assignable_rec(actual, expected, types, &mut seen)
}

fn assignable_rec(
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
    if let Type::Refined { original, ty } = expected.without_aliases() {
        return assignable_rec(actual, original, types, seen)
            && assignable_rec(actual, ty, types, seen);
    }
    if let Type::Refined { original, ty } = actual.without_aliases() {
        return assignable_rec(original, expected, types, seen)
            || assignable_rec(ty, expected, types, seen);
    }
    if same_alias_instance(actual, expected) {
        return true;
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
        return ms.iter().any(|m| assignable_rec(actual, m, types, seen));
    }
    if drops_readonly(actual, expected) {
        return false;
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
    if matches!(actual, Type::Error) || matches!(expected, Type::Error) {
        return true;
    }
    if matches!(actual, Type::Never) {
        return true;
    }
    // `void` is not a value: it has no runtime representation, so nothing else
    // satisfies it and it satisfies nothing else. Checked ahead of the
    // `TypeVar`/`Unknown` wildcards below, which would otherwise wave it into
    // a value slot and leave codegen to hit `value_type called on Void`.
    match (actual, expected) {
        (Type::Void, Type::Void) => return true,
        (Type::Void, _) | (_, Type::Void) => return false,
        _ => {}
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
        (Type::GenericParam { id: a, .. }, Type::GenericParam { id: b, .. }) => a == b,
        (Type::GenericParam { .. }, _) | (_, Type::GenericParam { .. }) => false,
        // Explicit before the `==` fallback, which would reject `StringLiteral("foo") <: String`.
        (Type::StringLiteral(a), Type::StringLiteral(b)) => a == b,
        (Type::StringLiteral(_) | Type::StringEnum { .. }, Type::String) => true,
        (Type::String, Type::StringLiteral(_)) => false,
        (Type::NumberLiteral(a), Type::NumberLiteral(b)) => a == b,
        (Type::NumberLiteral(_) | Type::NumberEnum { .. }, Type::Number) => true,
        (Type::Number, Type::NumberLiteral(_)) => false,
        // An enum's identity is its declaration: an aliased import
        // (`import { E as G }`) names the same enum under another `name`.
        (Type::NumberEnum { mangled: a, .. }, Type::NumberEnum { mangled: b, .. })
        | (Type::StringEnum { mangled: a, .. }, Type::StringEnum { mangled: b, .. }) => a == b,
        (Type::BooleanLiteral(a), Type::BooleanLiteral(b)) => a == b,
        (Type::BooleanLiteral(_), Type::Boolean) => true,
        (Type::Boolean, Type::BooleanLiteral(_)) => false,
        (Type::Array(ae), Type::Array(ee)) => assignable_rec(ae, ee, types, seen),
        (Type::Tuple(aa), Type::Tuple(ae)) => {
            aa.len() == ae.len()
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
            let (Some(class_form), Some(iface_form)) = (
                types.class_full_form(ma, aa),
                types.interface_full_form(me, ne, ae),
            ) else {
                return false;
            };
            if weak_type_rejects(&class_form, &iface_form) {
                return false;
            }
            iface_form
                .iter()
                .all(|(member, exp)| match class_form.get(member) {
                    Some(act) => {
                        !types.class_property_is_write_only(ma, aa, member)
                            && (exp.optional || !act.optional)
                            && types.class_member_assignable(ma, aa, member, &act.ty, &exp.ty, seen)
                    }
                    None => exp.optional,
                })
        }
        (Type::ClassRef { mangled, args, .. }, Type::Object { fields, index }) => {
            let Some(class_form) = types.class_full_form(mangled, args) else {
                return false;
            };
            if index.as_ref().is_some_and(|i| {
                class_form
                    .values()
                    .any(|f| !assignable_rec(&f.ty, &i.value, types, seen))
            }) {
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
                                &actual.ty,
                                &expected.ty,
                                seen,
                            )
                    }
                    None => expected.optional,
                })
        }
        // An interface value is not nominally a class.
        (Type::InterfaceRef { .. }, Type::ClassRef { .. }) => false,
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
            },
            Type::Function {
                params: pe,
                ret: re,
                predicate: predicate_e,
                has_rest: rest_e,
            },
        ) => {
            // A rest function can't stand for one whose own rest arguments
            // it would have to unpack.
            if *rest_e && !rest_a {
                return false;
            }
            // Predicate→non-predicate: ok (info dropped). Non-predicate→predicate: rejected (can't manufacture narrowing metadata).
            let predicate_ok = match (predicate_a, predicate_e) {
                (Some(a), Some(e)) => {
                    a.parameter_index == e.parameter_index
                        && assignable_rec(&a.asserted_type, &e.asserted_type, types, seen)
                }
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (None, None) => true,
            };
            let params_ok = if *rest_a && !rest_e {
                rest_function_accepts(pa, pe, |e, a| assignable_rec(e, a, types, seen))
            } else {
                Type::function_arity_fits(pa.len(), pe.len(), *rest_a)
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
                    || (ra.is_void() && matches!(re.peel(), Type::TypeVar(_)))
                    || assignable_rec(ra, re, types, seen))
        }
        // `readonly` is shallow (TS-faithful): it gates direct writes, not
        // assignability. Object width subtyping stays covariant per field.
        (
            Type::Object {
                fields: a_fields,
                index: a_index,
            },
            Type::Object {
                fields: e_fields,
                index: e_index,
            },
        ) => {
            if let Some(expected_index) = e_index {
                if !a_fields
                    .values()
                    .all(|f| assignable_rec(&f.ty, &expected_index.value, types, seen))
                {
                    return false;
                }
                if let Some(actual_index) = a_index
                    && !assignable_rec(&actual_index.value, &expected_index.value, types, seen)
                {
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
                    assignable_rec(&a_field.ty, &e_field.ty, types, seen)
                }
                None => e_field.optional,
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
            result
        }
        _ => actual == expected,
    }
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

/// Whether `actual` is a `readonly` array or tuple and `expected` a mutable one.
/// The element types still decide assignability everywhere else: `readonly` is
/// shallow and otherwise covariant, like the array it wraps.
pub(crate) fn drops_readonly(actual: &Type, expected: &Type) -> bool {
    actual.is_readonly_array()
        && !expected.is_readonly_array()
        && matches!(expected.peel(), Type::Array(_) | Type::Tuple(_))
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
    let Some((ma, _pa, na, aa)) = actual.interface_routing() else {
        return false;
    };
    let Some(expected_form) = types.interface_full_form(&me, ne, &ae) else {
        return false;
    };
    let Some(actual_form) = types.interface_full_form(&ma, na, &aa) else {
        return false;
    };
    if weak_type_rejects(&actual_form, &expected_form) {
        return false;
    }
    if let Some(index) = types.index_signature(expected) {
        if !matches!(
            actual.peel(),
            Type::Object { .. } | Type::ClassRef { .. } | Type::InterfaceRef { .. }
        ) {
            return false;
        }
        seen.push(key.clone());
        let compatible = actual_form
            .values()
            .all(|field| assignable_rec(&field.ty, &index.value, types, seen))
            && types
                .index_signature(actual)
                .is_none_or(|actual| assignable_rec(&actual.value, &index.value, types, seen));
        seen.pop();
        if !compatible {
            return false;
        }
    }
    seen.push(key);
    let ok = expected_form
        .iter()
        .all(|(member, exp)| match actual_form.get(member) {
            Some(act) => {
                (exp.optional || !act.optional) && assignable_rec(&act.ty, &exp.ty, types, seen)
            }
            None => exp.optional,
        });
    seen.pop();
    ok
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
        Type::Tuple(elements) | Type::Union(elements) => type_argument_depth(elements),
        Type::Function { params, ret, .. } => {
            type_argument_depth(params).max(type_nesting_depth(ret))
        }
        Type::Refined { original, ty } => type_nesting_depth(original).max(type_nesting_depth(ty)),
        Type::Object { fields, index } => fields
            .values()
            .map(|field| type_nesting_depth(&field.ty))
            .chain(index.iter().map(|i| type_nesting_depth(&i.value)))
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
    Some(Type::Object {
        index: types.index_signature(ty),
        fields: types.interface_data_shape(mangled, name, args)?,
    })
}

/// `Null` is excluded — it has its own narrowing path via `predicate_envs_eq_null`.
pub(super) fn literal_value_of(kind: &crate::TypedExprKind) -> Option<narrowing::LiteralValue> {
    use crate::TypedExprKind;
    match kind {
        TypedExprKind::Number(n) => Some(narrowing::LiteralValue::Number(
            crate::types::LiteralF64(*n),
        )),
        TypedExprKind::String(s) => Some(narrowing::LiteralValue::String(s.clone())),
        TypedExprKind::Boolean(b) => Some(narrowing::LiteralValue::Boolean(*b)),
        _ => None,
    }
}

pub(super) fn literal_to_type(lit: &narrowing::LiteralValue) -> Type {
    match lit {
        narrowing::LiteralValue::Number(n) => Type::NumberLiteral(*n),
        narrowing::LiteralValue::String(s) => Type::StringLiteral(s.clone()),
        narrowing::LiteralValue::Boolean(b) => Type::BooleanLiteral(*b),
    }
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
            },
        )
    }

    fn fn_ty(params: Vec<Type>, ret: Type) -> Type {
        Type::Function {
            params,
            ret: Box::new(ret),
            predicate: None,
            has_rest: false,
        }
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
        let pair = Type::Tuple(vec![Type::Number, Type::Number]);
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
                    generics: Vec::new(),
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
                    index: None,
                    generics: Vec::new(),
                    methods,
                    properties,
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
