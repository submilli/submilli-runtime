//! Method signatures retain binders that a bare function Type cannot express.
use std::collections::{BTreeMap, BTreeSet};

use super::assignable::{TypeResolver, assignable_rec};
use super::generic::{instantiate_method_signature, substitute_or_record, substitute_typevars};
use crate::type_size::TypeTooLarge;
use crate::typechecker::type_param_substitution::TypeParamSubstitution;
use crate::{MethodSig, ObjectField, Type, TypeKind};

impl TypeResolver<'_> {
    pub(super) fn method_relation(
        self,
        actual: &Type,
        expected: &Type,
        member: &str,
        seen: &mut Vec<(Type, Type)>,
    ) -> Option<bool> {
        let source = self.bound_method(actual, member)?;
        let target = self.bound_method(expected, member)?;
        if source.generics.is_empty()
            && source.overloads.is_empty()
            && target.generics.is_empty()
            && target.overloads.is_empty()
        {
            return None;
        }
        if !target.optional && source.optional {
            return Some(false);
        }
        for expected in target.signatures() {
            if !source
                .signatures()
                .any(|actual| self.signature_assignable(&actual, &expected, seen))
            {
                return Some(false);
            }
        }
        Some(true)
    }

    pub(super) fn required_methods_satisfied(
        self,
        actual: &Type,
        expected: &Type,
        seen: &mut Vec<(Type, Type)>,
    ) -> bool {
        let Type::InterfaceRef { mangled, name, .. } = expected.peel() else {
            return true;
        };
        let Some(symbol) = self.lookup(mangled, name) else {
            return false;
        };
        let TypeKind::Interface { methods, .. } = &symbol.kind else {
            return true;
        };
        methods.iter().all(|(member, signature)| {
            if signature.generics.is_empty() && signature.overloads.is_empty() {
                return true;
            }
            match self.method_relation(actual, expected, member, seen) {
                Some(result) => result,
                None => signature.optional,
            }
        })
    }

    fn bound_method(self, receiver: &Type, member: &str) -> Option<MethodSig> {
        self.bound_method_at(receiver, member, 0)
    }

    fn bound_method_at(self, receiver: &Type, member: &str, depth: u32) -> Option<MethodSig> {
        if depth >= crate::compiler_limits::MAX_TYPE_DEPTH {
            self.limits.record(TypeTooLarge::Depth);
            return None;
        }
        if let Type::Object { fields, .. } = receiver.peel() {
            let field = fields.get(member)?;
            return self.callable_signature(&field.ty, field.optional, depth + 1);
        }
        let (mangled, _, name, args) = receiver.interface_routing()?;
        let symbol = self.lookup(&mangled, name)?;
        if let TypeKind::Interface {
            methods,
            properties,
            generics,
            ..
        } = &symbol.kind
        {
            let bindings = generics.iter().cloned().zip(args).collect();
            let signature = methods.get(member).cloned().or_else(|| {
                properties.get(member).and_then(|property| {
                    self.callable_signature(&property.ty, property.optional, depth + 1)
                })
            })?;
            return self.limits.ok_or_record(instantiate_method_signature(
                &signature,
                &bindings,
                self.limits,
            ));
        }
        let mut found = None;
        let mut shadowed = false;
        super::classes::for_each_class_in_chain(
            |m| self.sym_by_mangled(m).cloned(),
            self.limits,
            &mangled,
            &args,
            |symbol, bindings| {
                if found.is_some() || shadowed {
                    return;
                }
                if let TypeKind::Class {
                    methods,
                    method_visibility,
                    fields,
                    ..
                } = &symbol.kind
                {
                    if method_visibility.get(member) == Some(&crate::Visibility::Private)
                        || fields
                            .get(member)
                            .is_some_and(|field| field.visibility == crate::Visibility::Private)
                    {
                        shadowed = true;
                        return;
                    }
                    if let Some(method) = methods.get(member) {
                        found = self.limits.ok_or_record(instantiate_method_signature(
                            method,
                            bindings,
                            self.limits,
                        ));
                    } else if let Some(field) = fields.get(member) {
                        // A field shadows a base method; its ordinary function type
                        // is checked by the structural member comparison instead.
                        shadowed = true;
                        if let Some(signature) =
                            self.callable_signature(&field.ty, field.optional, depth + 1)
                        {
                            found = self.limits.ok_or_record(instantiate_method_signature(
                                &signature,
                                bindings,
                                self.limits,
                            ));
                        }
                    }
                }
            },
        )?;
        found
    }

    /// The constraints on a callable value beyond its call signatures.
    /// Generic non-call methods stay nominal rather than losing their binders.
    pub(super) fn callable_value_constraint(self, ty: &Type) -> Type {
        match ty.peel() {
            Type::Function { .. } => Type::Unknown,
            Type::Intersection(members) => Type::intersection(
                members
                    .iter()
                    .map(|member| self.callable_value_constraint(member))
                    .collect(),
            ),
            Type::InterfaceRef {
                mangled,
                name,
                args,
                ..
            } => {
                let Some(symbol) = self.lookup(mangled, name) else {
                    return ty.clone();
                };
                let TypeKind::Interface {
                    generics,
                    methods,
                    call,
                    properties,
                    ..
                } = &symbol.kind
                else {
                    return ty.clone();
                };
                if call.is_none()
                    || methods
                        .values()
                        .any(|sig| sig.signatures().any(|sig| !sig.generics.is_empty()))
                {
                    return ty.clone();
                }
                let bindings = generics.iter().cloned().zip(args.iter().cloned()).collect();
                let mut fields = BTreeMap::new();
                for (name, sig) in methods {
                    if !self.limits.spend_work(1) {
                        return Type::Error;
                    }
                    fields.insert(
                        name.clone(),
                        ObjectField {
                            ty: self.method_type(sig, &bindings),
                            optional: sig.optional,
                            readonly: true,
                            method: true,
                        },
                    );
                }
                // Remove only this value's call signature. Nested callable
                // fields retain their complete contracts and nominal back-edges.
                for (name, sig) in properties {
                    if !self.limits.spend_work(1) {
                        return Type::Error;
                    }
                    let property_ty = substitute_or_record(&sig.ty, &bindings, self.limits);
                    let (field_ty, optional) = match fields.remove(name) {
                        Some(method) => (
                            Type::intersection(vec![method.ty, property_ty]),
                            method.optional && sig.optional,
                        ),
                        None => (property_ty, sig.optional),
                    };
                    fields.insert(
                        name.clone(),
                        ObjectField {
                            ty: field_ty,
                            optional,
                            readonly: sig.readonly,
                            method: false,
                        },
                    );
                }
                let (index, more_indexes) =
                    crate::split_index_signatures(self.index_signatures(ty));
                if fields.is_empty() && index.is_none() {
                    Type::Unknown
                } else {
                    Type::Object {
                        fields,
                        index,
                        more_indexes,
                    }
                }
            }
            _ => ty.clone(),
        }
    }

    /// The call signature of the interface `receiver` names, its generic
    /// arguments substituted.
    fn bound_call_signature(self, receiver: &Type) -> Option<MethodSig> {
        let (mangled, _, name, args) = receiver.interface_routing()?;
        let TypeKind::Interface { call, generics, .. } = &self.lookup(&mangled, name)?.kind else {
            return None;
        };
        let bindings = generics.iter().cloned().zip(args).collect();
        self.limits.ok_or_record(instantiate_method_signature(
            call.as_ref()?,
            &bindings,
            self.limits,
        ))
    }

    pub(super) fn callable_method_signature(self, ty: &Type) -> Option<MethodSig> {
        self.callable_signature(ty, false, 0)
    }

    fn callable_signature(self, ty: &Type, optional: bool, depth: u32) -> Option<MethodSig> {
        if depth >= crate::compiler_limits::MAX_TYPE_DEPTH {
            self.limits.record(TypeTooLarge::Depth);
            return None;
        }
        if let Type::InterfaceRef { .. } = ty.peel() {
            let mut signature = self.bound_call_signature(ty)?;
            signature.optional |= optional;
            return Some(signature);
        }
        if let Type::Intersection(members) = ty.peel() {
            let mut result: Option<MethodSig> = None;
            for member in members {
                if let Some(signature) = self.callable_signature(member, optional, depth + 1) {
                    if let Some(result) = &mut result {
                        result.append_signatures(signature);
                    } else {
                        result = Some(signature);
                    }
                }
            }
            return result;
        }
        MethodSig::from_function_type(ty, optional)
    }

    fn signature_assignable(
        self,
        actual: &MethodSig,
        expected: &MethodSig,
        seen: &mut Vec<(Type, Type)>,
    ) -> bool {
        self.limits
            .ok_or_record(self.limits.compare_methods(actual, expected, || {
                self.compare_signatures(actual, expected, seen)
            }))
            .unwrap_or(false)
    }

    fn compare_signatures(
        self,
        actual: &MethodSig,
        expected: &MethodSig,
        seen: &mut Vec<(Type, Type)>,
    ) -> Result<bool, TypeTooLarge> {
        let actual_ty = self.method_type(actual, &BTreeMap::new());
        let expected_ty = self.method_type(expected, &BTreeMap::new());
        // Fresh opaque target parameters express the promise of every target
        // instantiation. Avoid all identities in the compared types and bounds.
        let mut occupied: BTreeSet<u32> = self.generic_constraints.keys().copied().collect();
        let mut pending = vec![&actual_ty, &expected_ty];
        pending.extend(self.generic_constraints.values());
        pending.extend(actual.generics.constraints.values());
        pending.extend(expected.generics.constraints.values());
        while let Some(ty) = pending.pop() {
            if !self.limits.spend_work(1) {
                return Err(TypeTooLarge::Work);
            }
            if let Type::GenericParam { id, .. } = ty {
                occupied.insert(*id);
            }
            crate::type_size::for_each_child(ty, |child| pending.push(child));
        }
        let mut bindings = BTreeMap::new();
        let mut constraints = self.generic_constraints.clone();
        let mut next_id = 0u32;
        for name in &expected.generics {
            while occupied.contains(&next_id) {
                if !self.limits.spend_work(1) {
                    return Err(TypeTooLarge::Work);
                }
                next_id = next_id.checked_add(1).ok_or(TypeTooLarge::Work)?;
            }
            let id = next_id;
            occupied.insert(id);
            bindings.insert(
                name.clone(),
                Type::GenericParam {
                    id,
                    name: name.clone(),
                },
            );
        }
        for (name, bound) in expected.generics.constraints.iter() {
            if let Some(Type::GenericParam { id, .. }) = bindings.get(name) {
                constraints.insert(*id, substitute_typevars(bound, &bindings, self.limits)?);
            }
        }
        let expected_ty = substitute_typevars(&expected_ty, &bindings, self.limits)?;
        let resolver = TypeResolver {
            generic_constraints: &constraints,
            ..self
        };
        if actual.generics.is_empty() {
            return Ok(assignable_rec(&actual_ty, &expected_ty, resolver, seen));
        }
        let mut inferred = TypeParamSubstitution::new();
        let _ = inferred.unify(&actual_ty, &expected_ty, resolver);
        let mut unused = BTreeSet::new();
        for name in &actual.generics {
            if inferred.get(name).is_none() {
                unused.insert(name.clone());
                inferred.insert(
                    name.clone(),
                    actual
                        .generics
                        .constraints
                        .get(name)
                        .cloned()
                        .unwrap_or(Type::Unknown),
                );
            }
        }
        for (name, bound) in actual.generics.constraints.iter() {
            if unused.contains(name) {
                continue;
            }
            let argument = inferred.apply(&Type::TypeVar(name.clone()), self.limits)?;
            let bound = inferred.apply(bound, self.limits)?;
            if !assignable_rec(&argument, &bound, resolver, seen) {
                return Ok(false);
            }
        }
        Ok(assignable_rec(
            &inferred.apply(&actual_ty, self.limits)?,
            &expected_ty,
            resolver,
            seen,
        ))
    }
}
