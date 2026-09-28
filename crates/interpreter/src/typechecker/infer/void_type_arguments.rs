//! Reject type substitutions that put `void` in a value slot, while allowing
//! callback and method returns to have no result.

use super::assignable::TypeResolver;
use crate::typechecker::type_param_substitution::TypeParamSubstitution;
use crate::{MangledName, Type, TypeKind};
use std::collections::BTreeSet;

pub(super) fn invalid_argument<'t>(
    ty: &'t Type,
    allow_void: bool,
    types: TypeResolver<'_>,
) -> Option<&'t Type> {
    // A legal nested void must not hide a later forbidden never argument.
    let offender = super::void_value::valueless_within_type_argument(ty, false)
        .or_else(|| super::void_value::valueless_within_type_argument(ty, true))?;
    if offender.is_void() && invalid_position(ty, allow_void, types).is_none() {
        return None;
    }
    Some(offender)
}

pub(super) fn invalid_position(
    ty: &Type,
    return_position: bool,
    types: TypeResolver<'_>,
) -> Option<String> {
    Scan {
        types,
        active: BTreeSet::new(),
    }
    .visit(ty, return_position)
}

struct Scan<'a> {
    types: TypeResolver<'a>,
    // Valid non-void arguments have value representations even when nested
    // callbacks return void. Only bare void can move into an invalid position
    // as recursive declarations wrap or permute their type parameters.
    active: BTreeSet<(MangledName, Vec<bool>, bool)>,
}

impl Scan<'_> {
    fn visit(&mut self, ty: &Type, return_position: bool) -> Option<String> {
        match ty.peel() {
            Type::Void if !return_position => Some("a value slot".to_string()),
            Type::Function { params, ret, .. } => params
                .iter()
                .find_map(|p| self.visit(p, false))
                .or_else(|| self.visit(ret, true)),
            Type::Array(elem) => self.visit(elem, false),
            Type::Tuple(elements) | Type::Union(elements) => {
                elements.iter().find_map(|t| self.visit(t, false))
            }
            Type::Object { fields, index } => index
                .as_ref()
                .and_then(|i| self.visit(&i.value, false))
                .or_else(|| {
                    fields.iter().find_map(|(name, field)| {
                        self.visit(&field.ty, false)
                            .map(|where_| format!("property `{name}` ({where_})"))
                    })
                }),
            Type::InterfaceRef {
                mangled,
                name,
                args,
                ..
            } => self.interface(mangled, name, args),
            Type::ClassRef { args, .. } => args.iter().find_map(|t| self.visit(t, false)),
            Type::AliasRef {
                mangled,
                name,
                args,
                ..
            } => {
                let key = (
                    mangled.clone(),
                    args.iter().map(Type::is_void).collect(),
                    return_position,
                );
                if !self.active.insert(key.clone()) {
                    return None;
                }
                let result = self.alias(mangled, name, args, return_position);
                self.active.remove(&key);
                result
            }
            _ => None,
        }
    }

    fn interface(&mut self, mangled: &MangledName, name: &str, args: &[Type]) -> Option<String> {
        if !args.iter().any(contains_void) {
            return None;
        }
        // A recursive edge can transform T into T[] or another value-bearing
        // composite. Check that transformation before stopping the member walk.
        if let Some(position) = args.iter().find_map(|arg| self.visit(arg, true)) {
            return Some(position);
        }
        let key = (
            mangled.clone(),
            args.iter().map(Type::is_void).collect(),
            false,
        );
        if !self.active.insert(key.clone()) {
            return None;
        }
        let result = self.interface_members(mangled, name, args);
        self.active.remove(&key);
        result
    }

    fn interface_members(
        &mut self,
        mangled: &MangledName,
        name: &str,
        args: &[Type],
    ) -> Option<String> {
        let TypeKind::Interface {
            generics,
            methods,
            properties,
            ..
        } = &self.types.lookup(mangled, name)?.kind
        else {
            return None;
        };
        let sub = TypeParamSubstitution::from_pairs(generics, args);
        for (member, property) in properties {
            if let Some(position) = self.visit(&sub.apply(&property.ty), false) {
                return Some(format!("property `{name}.{member}` ({position})"));
            }
        }
        for (member, method) in methods {
            // Method type parameters shadow an interface parameter of the same name.
            let outer: Vec<_> = generics
                .iter()
                .zip(args)
                .filter(|(g, _)| !method.generics.contains(g))
                .collect();
            let sub = TypeParamSubstitution::from_pairs(
                &outer.iter().map(|(g, _)| (*g).clone()).collect::<Vec<_>>(),
                &outer.iter().map(|(_, a)| (*a).clone()).collect::<Vec<_>>(),
            );
            let invalid = method
                .params
                .iter()
                .find_map(|p| self.visit(&sub.apply(&p.ty), false))
                .or_else(|| self.visit(&sub.apply(&method.ret), true));
            if let Some(position) = invalid {
                return Some(format!("method `{name}.{member}` ({position})"));
            }
        }
        None
    }

    fn alias(
        &mut self,
        mangled: &MangledName,
        name: &str,
        args: &[Type],
        return_position: bool,
    ) -> Option<String> {
        let TypeKind::Alias { generics, ty, .. } = &self.types.lookup(mangled, name)?.kind else {
            return None;
        };
        self.visit(
            &TypeParamSubstitution::from_pairs(generics, args).apply(ty),
            return_position,
        )
    }
}

fn contains_void(ty: &Type) -> bool {
    match ty.peel() {
        Type::Void => true,
        Type::Array(element) => contains_void(element),
        Type::Tuple(elements) | Type::Union(elements) => elements.iter().any(contains_void),
        Type::Function { params, ret, .. } => {
            params.iter().any(contains_void) || contains_void(ret)
        }
        Type::Object { fields, index } => {
            index.as_ref().is_some_and(|i| contains_void(&i.value))
                || fields.values().any(|field| contains_void(&field.ty))
        }
        Type::InterfaceRef { args, .. }
        | Type::ClassRef { args, .. }
        | Type::AliasRef { args, .. } => args.iter().any(contains_void),
        _ => false,
    }
}
