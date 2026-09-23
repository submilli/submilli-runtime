//! Type-alias body resolution.
//!
//! Names are forward-declared in the signatures pre-pass; bodies are
//! resolved lazily here the first time a reference is encountered. That
//! ordering is what lets an alias reference any other type regardless of
//! source order, and lets a recursive alias close its cycle through a
//! lazy [`Type::AliasRef`] back-edge instead of an infinite inline body.

use std::collections::BTreeSet;

use crate::mangle::MangledName;
use crate::{ObjectField, Type, TypeAnnotation, TypeKind};

use super::assignable::TypeResolver;

use super::Inferer;

/// Does `ty` contain a [`Type::AliasRef`] anywhere reachable without
/// descending into another alias's stored body? Cheap pre-check so
/// [`rehydrate_alias_refs`] only rebuilds types that need it (the
/// overwhelmingly common no-recursive-alias case is one walk, no allocation).
pub(crate) fn type_has_alias_ref(ty: &Type) -> bool {
    let mut found = false;
    walk_alias_refs(ty, &mut |_| found = true);
    found
}

/// Names every back-edge [`rehydrate_alias_refs`] would expand in `ty`.
pub(crate) fn alias_ref_names(ty: &Type) -> BTreeSet<MangledName> {
    let mut names = BTreeSet::new();
    walk_alias_refs(ty, &mut |mangled| {
        names.insert(mangled.clone());
    });
    names
}

/// Visits every [`Type::AliasRef`] reachable in `ty` without descending into
/// another alias's stored body — the same reach [`rehydrate_alias_refs`] has.
fn walk_alias_refs(ty: &Type, visit: &mut dyn FnMut(&MangledName)) {
    match ty {
        Type::AliasRef { mangled, .. } => visit(mangled),
        Type::Union(ms) | Type::Tuple(ms) => {
            for m in ms {
                walk_alias_refs(m, visit);
            }
        }
        Type::Array(e) | Type::Readonly(e) => walk_alias_refs(e, visit),
        Type::Object { fields } => {
            for f in fields.values() {
                walk_alias_refs(&f.ty, visit);
            }
        }
        Type::Function {
            params,
            ret,
            predicate,
            ..
        } => {
            for p in params {
                walk_alias_refs(p, visit);
            }
            walk_alias_refs(ret, visit);
            if let Some(p) = predicate {
                walk_alias_refs(&p.asserted_type, visit);
            }
        }
        // A carrying `Alias`'s args may hold an `AliasRef`; its stored
        // body legitimately does (the back-edge) and is left alone.
        Type::InterfaceRef { args, .. } | Type::Alias { args, .. } => {
            for a in args {
                walk_alias_refs(a, visit);
            }
        }
        _ => {}
    }
}

impl<'a> Inferer<'a> {
    /// Resolve a *reference* to type alias `name` at a use site.
    ///
    /// - If `name` is already mid-resolution (on the resolution stack),
    ///   this is a recursion back-edge → return a lazy
    ///   [`Type::AliasRef`] (no inline body, keeping the carrying type
    ///   finite).
    /// - Otherwise ensure the body is resolved (driving forward / mutual
    ///   references), then inline it as `Type::Alias { name, args, ty }`
    ///   so the label flows through `Display` while every other consumer
    ///   peels to the structural body.
    pub(super) fn resolve_alias_reference(
        &mut self,
        name: &str,
        arg_annots: &[TypeAnnotation],
        span: crate::Span,
    ) -> Type {
        // Arity check against the forward-declared placeholder's
        // generic-param list (known before the body is resolved).
        let (generics, mangled): (Vec<String>, crate::MangledName) = match self.types.lookup(name) {
            Some(sym) => match &sym.kind {
                TypeKind::Alias { generics, .. } => (generics.clone(), sym.mangled_name.clone()),
                _ => return Type::Error,
            },
            None => return Type::Error,
        };
        let alias_package = self.type_package(name);
        if arg_annots.len() != generics.len() {
            let plural = if generics.len() == 1 {
                "argument"
            } else {
                "arguments"
            };
            self.error(
                span,
                format!(
                    "type alias `{}` expects {} type {}, got {}",
                    name,
                    generics.len(),
                    plural,
                    arg_annots.len(),
                ),
            );
            return Type::Error;
        }
        let resolved_args: Vec<Type> = arg_annots.iter().map(|a| self.resolve_type(a)).collect();

        // Recursion back-edge: emit the lazy by-name reference.
        if self.alias_resolution_stack.iter().any(|n| n == name) {
            return Type::alias_ref(alias_package, name.to_string(), mangled, resolved_args);
        }

        // Ensure the body is resolved (no-op if already done).
        self.resolve_alias_body(name);
        let body = match self.types.lookup(name) {
            Some(sym) => match &sym.kind {
                TypeKind::Alias { ty, .. } => ty.clone(),
                _ => Type::Error,
            },
            None => Type::Error,
        };
        let body = if generics.is_empty() {
            body
        } else {
            let sub =
                crate::typechecker::type_param_substitution::TypeParamSubstitution::from_pairs(
                    &generics,
                    &resolved_args,
                );
            sub.apply(&body)
        };
        Type::alias_ty(
            alias_package,
            name.to_string(),
            mangled,
            resolved_args,
            Box::new(body),
        )
    }

    /// Resolve alias `name`'s body, fill in its placeholder symbol, and
    /// mirror it onto the typed AST. No-op if already resolved. Resolves
    /// in an isolated generic scope (only the alias's own params), so a
    /// reference triggered from inside an interface / function signature
    /// doesn't leak that context's generics into the alias body.
    pub(super) fn resolve_alias_body(&mut self, name: &str) {
        let Some(pending) = self.pending_aliases.remove(name) else {
            return;
        };
        self.alias_resolution_stack.push(name.to_string());
        let saved_generics = std::mem::take(&mut self.generics_in_scope);
        let saved_bodies = std::mem::take(&mut self.body_instantiations);
        self.push_signature_generics(pending.generics.clone());
        let resolved_body = self.resolve_type(&pending.annotation);
        self.pop_signature_generics();
        self.generics_in_scope = saved_generics;
        self.body_instantiations = saved_bodies;
        self.alias_resolution_stack.pop();

        let mut final_symbol = None;
        if let Some(sym) = self.types.lookup_mut(name)
            && let TypeKind::Alias { ty, .. } = &mut sym.kind
        {
            *ty = resolved_body.clone();
            final_symbol = Some(sym.clone());
        }
        let typed_decl = crate::TypedTypeDecl::Alias(crate::TypedTypeAliasDecl {
            name: pending.name,
            generics: pending.generics,
            ty: resolved_body,
            doc: pending.doc,
        });
        let Some(symbol) = final_symbol else {
            return;
        };
        self.add_typed_type_decl(typed_decl, symbol);
    }

    /// [`rehydrate_alias_refs`] against this inferer's type registry.
    pub(super) fn rehydrate_alias_refs(&self, ty: &Type) -> Type {
        rehydrate_alias_refs(ty, self.resolver())
    }
}

/// Replace every recursion back-edge ([`Type::AliasRef`]) reachable in `ty`
/// with the rehydrated inline [`Type::Alias`] form (body resolved by name, one
/// level — the body keeps its own nested back-edges). Types reaching codegen
/// must be `AliasRef`-free so it lowers them consistently: a bare `AliasRef`
/// lowers to the universal `$Object`, but the equivalent `Alias`-to-object
/// peels to the narrower `$ObjectShape`, and the two must not collide in a
/// slot. Applied to every `infer_expr` result and to every type a generic
/// parameter binds to (guarded by [`type_has_alias_ref`] at the hot call site,
/// so the common case stays a cheap walk).
pub(crate) fn rehydrate_alias_refs(ty: &Type, types: TypeResolver<'_>) -> Type {
    rehydrate_alias_refs_skipping(ty, types, &BTreeSet::new())
}

/// [`rehydrate_alias_refs`], leaving a back-edge to any alias named in `skip`
/// as it is.
///
/// Expansion is one level, so a walk that follows the result and expands again
/// only terminates while each level re-spells the *same* back-edge. Under
/// polymorphic recursion (`type G<T> = { v: T; next: G<G<T>> }`) every level's
/// argument is strictly larger, so a caller that walks what it expands has to
/// name the aliases already open on its path here, or it will not stop.
pub(crate) fn rehydrate_alias_refs_skipping(
    ty: &Type,
    types: TypeResolver<'_>,
    skip: &BTreeSet<MangledName>,
) -> Type {
    // Nothing to rebuild, and rebuilding anyway would re-canonicalize every
    // union it walks through — so the cheap check is a correctness guard as
    // much as an optimization, and belongs here rather than at each caller.
    if !type_has_alias_ref(ty) {
        return ty.clone();
    }
    rehydrate_reachable(ty, types, skip)
}

/// The body a recursion back-edge names, with `args` substituted for the
/// alias's own generics. `Type::Error` when the name doesn't resolve to an
/// alias, which keeps a walk finite rather than looping on an unresolvable name.
///
/// A back-edge is a *name*, not a body: `Type::union` cannot flatten through it
/// and `Type::peel` cannot see past it, so a probe that stops there reads
/// `type J = number | null | Wrap[]` as opaque and answers "not nullable" for a
/// type spelling `null` one name away. Anything asking a structural question of
/// a back-edge has to come through here — or, where there is no type table to
/// resolve against (codegen's `may_hold_null`, `emit_cast_to`), assume the
/// widest answer.
///
/// Structural resolution: a library-returned recursive alias is resolvable by
/// its own package even when the consumer never imported the alias name.
pub(crate) fn alias_ref_body(
    mangled: &MangledName,
    name: &str,
    args: &[Type],
    types: TypeResolver<'_>,
) -> Type {
    let Some(sym) = types.lookup(mangled, name) else {
        return Type::Error;
    };
    let TypeKind::Alias {
        generics, ty: body, ..
    } = &sym.kind
    else {
        return Type::Error;
    };
    if generics.is_empty() {
        return body.clone();
    }
    crate::typechecker::type_param_substitution::TypeParamSubstitution::from_pairs(generics, args)
        .apply(body)
}

fn rehydrate_reachable(ty: &Type, types: TypeResolver<'_>, skip: &BTreeSet<MangledName>) -> Type {
    match ty {
        Type::AliasRef { mangled, .. } if skip.contains(mangled) => ty.clone(),
        Type::AliasRef {
            mangled,
            package,
            name,
            args,
        } => {
            let rehydrated_args: Vec<Type> = args
                .iter()
                .map(|a| rehydrate_reachable(a, types, skip))
                .collect();
            let body = alias_ref_body(mangled, name, &rehydrated_args, types);
            Type::alias_ty(
                package.clone(),
                name.clone(),
                mangled.clone(),
                rehydrated_args,
                Box::new(body),
            )
        }
        Type::Union(ms) => Type::union(
            ms.iter()
                .map(|m| rehydrate_reachable(m, types, skip))
                .collect(),
        ),
        Type::Array(e) => Type::Array(Box::new(rehydrate_reachable(e, types, skip))),
        Type::Readonly(e) => Type::Readonly(Box::new(rehydrate_reachable(e, types, skip))),
        Type::Tuple(es) => Type::Tuple(
            es.iter()
                .map(|e| rehydrate_reachable(e, types, skip))
                .collect(),
        ),
        Type::Object { fields } => Type::Object {
            fields: fields
                .iter()
                .map(|(k, f)| {
                    (
                        k.clone(),
                        ObjectField {
                            ty: rehydrate_reachable(&f.ty, types, skip),
                            optional: f.optional,
                            readonly: f.readonly,
                        },
                    )
                })
                .collect(),
        },
        Type::Function {
            params,
            ret,
            predicate,
            has_rest,
        } => Type::Function {
            params: params
                .iter()
                .map(|p| rehydrate_reachable(p, types, skip))
                .collect(),
            ret: Box::new(rehydrate_reachable(ret, types, skip)),
            predicate: predicate.as_ref().map(|p| {
                Box::new(crate::TypePredicate {
                    parameter_index: p.parameter_index,
                    asserted_type: rehydrate_reachable(&p.asserted_type, types, skip),
                })
            }),
            has_rest: *has_rest,
        },
        Type::InterfaceRef {
            mangled,
            package,
            name,
            args,
        } => Type::interface_ref(
            package.clone(),
            name.clone(),
            mangled.clone(),
            args.iter()
                .map(|a| rehydrate_reachable(a, types, skip))
                .collect(),
        ),
        // Carrying alias: rehydrate args, but leave the stored body —
        // its `AliasRef` leaves are the legitimate finite encoding.
        Type::Alias {
            mangled,
            package,
            name,
            args,
            ty: body,
        } => Type::alias_ty(
            package.clone(),
            name.clone(),
            mangled.clone(),
            args.iter()
                .map(|a| rehydrate_reachable(a, types, skip))
                .collect(),
            body.clone(),
        ),
        _ => ty.clone(),
    }
}
