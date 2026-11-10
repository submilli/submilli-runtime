//! Type-alias body resolution.
//!
//! Names are forward-declared in the signatures pre-pass; bodies are
//! resolved lazily here the first time a reference is encountered. That
//! ordering is what lets an alias reference any other type regardless of
//! source order, and lets a recursive alias close its cycle through a
//! lazy [`Type::AliasRef`] back-edge instead of an infinite inline body.

use crate::compiler_error::CompilerFailure;
use crate::lower_local_types::{display_type_name, written_type_args};
use crate::type_size::{TypeBudget, TypeTooLarge, map_children};

use std::collections::BTreeSet;

use crate::mangle::MangledName;
use crate::{Type, TypeAnnotation, TypeKind};

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
        Type::Tuple(ms) => {
            for m in ms {
                walk_alias_refs(m, visit);
            }
        }
        Type::Union(ms) => {
            for m in ms {
                walk_alias_refs(m, visit);
            }
        }
        Type::Array(e) | Type::Readonly(e) | Type::KeyOf(e) => walk_alias_refs(e, visit),
        Type::IndexedAccess { object, index } => {
            walk_alias_refs(object, visit);
            walk_alias_refs(index, visit);
        }
        Type::Object { fields, index } => {
            if let Some(i) = index {
                walk_alias_refs(&i.value, visit);
            }
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

/// See [`Inferer::pending_declaration`].
pub(super) enum PendingDeclaration {
    /// The operand names the declaration being resolved or bound: the name.
    Cycle(String),
    /// The operand names a declaration bound later in the signature pass.
    Forward,
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
    ) -> Result<Type, CompilerFailure> {
        // Arity check against the forward-declared placeholder's
        // generic-param list (known before the body is resolved).
        let (generics, mangled): (crate::GenericParameters, crate::MangledName) = match self
            .types
            .lookup(name)
        {
            Some(sym) => match &sym.kind {
                TypeKind::Alias { generics, .. } => (generics.clone(), sym.mangled_name.clone()),
                _ => return Ok(Type::Error),
            },
            None => return Ok(Type::Error),
        };
        let alias_package = self.type_package(name);
        if arg_annots.len() != generics.len() {
            let expected = written_type_args(name, &generics).len();
            let plural = if expected == 1 {
                "argument"
            } else {
                "arguments"
            };
            self.error(
                span,
                format!(
                    "type alias `{}` expects {} type {}, got {}",
                    display_type_name(name),
                    expected,
                    plural,
                    written_type_args(name, arg_annots).len(),
                ),
            );
            return Ok(Type::Error);
        }
        let resolved_args: Vec<Type> = arg_annots
            .iter()
            .map(|a| self.resolve_type(a))
            .collect::<Result<_, _>>()?;

        if let Some(utility) = super::utility_types::utility(&mangled) {
            return self.resolve_utility_application(utility, resolved_args, span);
        }

        // Recursion back-edge: emit the lazy by-name reference.
        if self.alias_resolution_stack.iter().any(|n| n == name) {
            return Ok(Type::alias_ref(
                alias_package,
                name.to_string(),
                mangled,
                resolved_args,
            ));
        }

        // Ensure the body is resolved (no-op if already done).
        self.resolve_alias_body(name)?;
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
                crate::typechecker::type_param_substitution::TypeParamSubstitution::instantiation(
                    &generics,
                    &resolved_args,
                );
            let body = sub
                .apply(&body, &self.type_limits)
                .map_err(super::type_limit_at(span))?;
            // `type V<T, K extends keyof T> = T[K]` evaluates once its
            // arguments are known.
            self.normalize_at(&body, span)?
        };
        let alias = Type::alias_ty(
            alias_package,
            name.to_string(),
            mangled,
            resolved_args,
            Box::new(body),
        );
        // The alias keeps its arguments beside the substituted body, so even an
        // identity alias doubles the type it wraps.
        crate::type_size::check(&alias).map_err(super::type_limit_at(span))?;
        Ok(alias)
    }

    /// A written utility application, `Partial<User>`: its constraints are
    /// checked here, at the application's own span, and it evaluates to the
    /// type it stands for.
    fn resolve_utility_application(
        &mut self,
        utility: super::utility_types::Utility,
        args: Vec<Type>,
        span: crate::Span,
    ) -> Result<Type, CompilerFailure> {
        use super::utility_types::{Evaluated, Utility, deferred, evaluate, label};
        // `typeof identity` names a generic function, whose own type
        // parameters are no parameter in scope here. tsc instantiates them
        // with their constraints for `ReturnType` and `Parameters`; the
        // function type no longer records those, so they become `unknown`.
        let args = if matches!(utility, Utility::Parameters | Utility::ReturnType) {
            args.iter()
                .map(|arg| self.instantiate_foreign_type_vars(arg))
                .collect()
        } else {
            args
        };
        let deferred = deferred(utility, args.clone());
        self.check_application_constraints(&deferred, span)?;
        if let Some(ty) = self.defer_if_pending(utility.name(), &args, span, || deferred.clone()) {
            return Ok(ty);
        }
        let ty = match evaluate(utility, &args, self.resolver()) {
            Evaluated::Resolved(ty) => label(utility, args, ty),
            // It evaluates once instantiation substitutes its type parameters.
            Evaluated::Deferred => deferred,
            Evaluated::Invalid(Some(reason)) => {
                self.error(span, reason);
                Type::Error
            }
            Evaluated::Invalid(None) => Type::Error,
        };
        crate::type_size::check(&ty).map_err(super::type_limit_at(span))?;
        Ok(ty)
    }

    /// `ty` with each type variable that names no type parameter in scope
    /// replaced by `unknown`.
    fn instantiate_foreign_type_vars(&self, ty: &Type) -> Type {
        match ty {
            Type::TypeVar(name) if !self.is_generic_in_scope(name) => Type::Unknown,
            _ => crate::type_size::map_children_infallible(ty, |child| {
                self.instantiate_foreign_type_vars(child)
            }),
        }
    }

    /// Run `bind` for the interface or class `name`: a form over it there is
    /// a cycle, and afterwards it is bound.
    pub(super) fn with_binding<T>(
        &mut self,
        name: String,
        bind: impl FnOnce(&mut Self) -> Result<T, CompilerFailure>,
    ) -> Result<T, CompilerFailure> {
        self.binding_types.push(name.clone());
        let result = bind(self);
        self.binding_types.pop();
        self.unbound_types.remove(&name);
        result
    }

    /// The form a written operator stands for while `operands` name a
    /// declaration not bound yet: `Error` after reporting a cycle, or `form`,
    /// evaluated where it is used once every declaration is bound. `None`
    /// when every declaration they name is bound.
    pub(super) fn defer_if_pending(
        &mut self,
        operator: &str,
        operands: &[Type],
        span: crate::Span,
        form: impl FnOnce() -> Type,
    ) -> Option<Type> {
        match self.pending_declaration(operands)? {
            PendingDeclaration::Cycle(declared) => {
                self.report_cycle(operator, &declared, span);
                Some(Type::Error)
            }
            PendingDeclaration::Forward => {
                let form = form();
                self.forward_forms.push((form.clone(), span));
                Some(form)
            }
        }
    }

    /// Whether `operands` name a declaration the signature pass has not
    /// finished: one it is resolving or binding now (a cycle, as in `type
    /// Tree = { kids: Partial<Tree>[] }` or `interface Node { next:
    /// Readonly<Node> }`), or a later one whose members are not bound yet.
    pub(super) fn pending_declaration(&self, operands: &[Type]) -> Option<PendingDeclaration> {
        let mut referenced = BTreeSet::new();
        let mut pending: Vec<&Type> = operands.iter().collect();
        while let Some(ty) = pending.pop() {
            if let Type::InterfaceRef { mangled, .. }
            | Type::ClassRef { mangled, .. }
            | Type::AliasRef { mangled, .. } = ty
            {
                referenced.insert(mangled.clone());
            }
            crate::type_size::for_each_child(ty, |child| pending.push(child));
        }
        let is_referenced = |name: &&String| {
            self.type_mangled_name(name)
                .is_some_and(|mangled| referenced.contains(&mangled))
        };
        if let Some(declared) = self.alias_resolution_stack.iter().find(is_referenced) {
            return Some(PendingDeclaration::Cycle(declared.clone()));
        }
        if let Some(declared) = self.binding_types.iter().find(is_referenced) {
            return Some(if self.resolving_method_signature {
                PendingDeclaration::Forward
            } else {
                PendingDeclaration::Cycle(declared.clone())
            });
        }
        self.unbound_types
            .iter()
            .find(is_referenced)
            .map(|_| PendingDeclaration::Forward)
    }

    /// A utility, `keyof` or `T[K]` (`operator`) over the type `declared`
    /// inside its own definition, which has no evaluation yet (SUB-2154).
    pub(super) fn report_cycle(&mut self, operator: &str, declared: &str, span: crate::Span) {
        self.error_with_help(
            span,
            format!(
                "`{operator}` cannot be applied to `{}` inside its own definition",
                display_type_name(declared),
            ),
            vec![format!(
                "spell out the type `{operator}` would give in a separate declaration; \
                 self-referencing applications are not supported yet"
            )],
        );
    }

    /// Check each form kept lazy over a later declaration now that every
    /// declaration is bound, reporting where it was written why it has no
    /// evaluation: a missing member (`B["zz"]`), or a cycle of forms that only
    /// name each other (`A["b"]` and `B["c"]`).
    pub(super) fn check_forward_forms(&mut self) -> Result<(), CompilerFailure> {
        let forms = std::mem::take(&mut self.forward_forms);
        if forms.is_empty() {
            return Ok(());
        }
        let _settled = super::utility_types::SettledForms::enter();
        for (form, span) in forms {
            let mut failures = Vec::new();
            super::utility_types::normalize_reporting(&form, self.resolver(), &mut failures)
                .map_err(super::type_limit_at(span))?;
            if let Some(failure) = failures.into_iter().next() {
                self.error(span, failure);
            }
        }
        Ok(())
    }

    /// Replace the forms kept lazy in interface and class members with what
    /// they evaluate to, now that every declaration is bound. A method's
    /// parameters and result then have one representation for its callers
    /// and its body: `static key(k: keyof Box): string { return k; }` passes
    /// a `string`, not an erased `keyof Box`.
    pub(super) fn settle_member_types(&mut self) -> Result<(), CompilerFailure> {
        let names: Vec<String> = self.types.iter_names().map(str::to_owned).collect();
        for name in names {
            let Some(symbol) = self.types.lookup(&name) else {
                continue;
            };
            if !matches!(
                symbol.kind,
                TypeKind::Interface { .. } | TypeKind::Class { .. }
            ) {
                continue;
            }
            let mut kind = symbol.kind.clone();
            let mut changed = false;
            let mut exceeded = None;
            for_each_member_type_mut(&mut kind, &mut |ty| {
                if !super::utility_types::has_evaluable_application(ty) {
                    return;
                }
                match super::utility_types::normalize(ty, self.resolver()) {
                    Ok(settled) => {
                        *ty = settled;
                        changed = true;
                    }
                    Err(limit) => exceeded = Some(limit),
                }
            });
            if let Some(limit) = exceeded {
                return Err(super::type_limit_unlocated(limit));
            }
            if changed && let Some(symbol) = self.types.lookup_mut(&name) {
                symbol.kind = kind;
            }
        }
        Ok(())
    }

    fn type_mangled_name(&self, name: &str) -> Option<crate::MangledName> {
        self.types.lookup(name).map(|sym| sym.mangled_name.clone())
    }

    /// Resolve alias `name`'s body, fill in its placeholder symbol, and
    /// mirror it onto the typed AST. No-op if already resolved. Resolves
    /// in an isolated generic scope (only the alias's own params), so a
    /// reference triggered from inside an interface / function signature
    /// doesn't leak that context's generics into the alias body.
    pub(super) fn resolve_alias_body(&mut self, name: &str) -> Result<(), CompilerFailure> {
        let Some(pending) = self.pending_aliases.remove(name) else {
            return Ok(());
        };
        self.alias_resolution_stack.push(name.to_string());
        let saved_generics = std::mem::take(&mut self.generics_in_scope);
        let saved_bodies = std::mem::take(&mut self.body_instantiations);
        let parameters = self.push_declared_generics(&pending.generics)?;
        let resolved_body = self.resolve_type(&pending.annotation)?;
        self.pop_signature_generics();
        self.generics_in_scope = saved_generics;
        self.body_instantiations = saved_bodies;
        self.alias_resolution_stack.pop();

        let mut final_symbol = None;
        if let Some(sym) = self.types.lookup_mut(name)
            && let TypeKind::Alias { ty, generics, .. } = &mut sym.kind
        {
            *ty = resolved_body.clone();
            *generics = parameters.clone();
            final_symbol = Some(sym.clone());
        }
        let typed_decl = crate::TypedTypeDecl::Alias(crate::TypedTypeAliasDecl {
            name: pending.name,
            generics: parameters,
            ty: resolved_body,
            doc: pending.doc,
        });
        let Some(symbol) = final_symbol else {
            return Ok(());
        };
        self.add_typed_type_decl(typed_decl, symbol)?;

        Ok(())
    }

    /// [`rehydrate_alias_refs`] against this inferer's type registry, with
    /// every concrete utility application, `keyof` and `T[K]` evaluated: a
    /// generic interface's `keyof T` member read at `J<{ a: number }>` is
    /// `"a"`.
    pub(super) fn rehydrate_and_evaluate(&self, ty: &Type) -> Result<Type, TypeTooLarge> {
        let rehydrated = rehydrate_alias_refs(ty, self.resolver())?;
        super::utility_types::normalize(&rehydrated, self.resolver())
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
pub(crate) fn rehydrate_alias_refs(
    ty: &Type,
    types: TypeResolver<'_>,
) -> Result<Type, TypeTooLarge> {
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
) -> Result<Type, TypeTooLarge> {
    // Nothing to rebuild, and rebuilding anyway would re-canonicalize every
    // union it walks through — so the cheap check is a correctness guard as
    // much as an optimization, and belongs here rather than at each caller.
    if !type_has_alias_ref(ty) {
        return Ok(ty.clone());
    }
    rehydrate_reachable(ty, types, skip, 1, &mut types.limits.budget())
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
    if let Some(utility) = super::utility_types::utility(mangled) {
        return utility_body(utility, args, types);
    }
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
    let substituted = types.limits.type_or_error(
        crate::typechecker::type_param_substitution::TypeParamSubstitution::instantiation(
            generics, args,
        )
        .apply(body, types.limits),
    );
    types
        .limits
        .type_or_error(super::utility_types::normalize(&substituted, types))
}

/// The type a lazy utility application stands for. A deferred application is
/// its own body, so the walk stops there.
fn utility_body(
    utility: super::utility_types::Utility,
    args: &[Type],
    types: TypeResolver<'_>,
) -> Type {
    use super::utility_types::{Evaluated, deferred, evaluate};
    match evaluate(utility, args, types) {
        Evaluated::Resolved(ty) => ty,
        Evaluated::Deferred => deferred(utility, args.to_vec()),
        Evaluated::Invalid(_) => Type::Error,
    }
}

/// Builds under a budget from `types`: expanding a back-edge copies a stored
/// alias body, which a non-generic alias would otherwise copy without bound.
fn rehydrate_reachable(
    ty: &Type,
    types: TypeResolver<'_>,
    skip: &BTreeSet<MangledName>,
    depth: u32,
    budget: &mut TypeBudget<'_>,
) -> Result<Type, TypeTooLarge> {
    let child = depth.saturating_add(1);
    match ty {
        Type::AliasRef { mangled, .. } if skip.contains(mangled) => {
            budget.charge_copy(ty, depth)?;
            Ok(ty.clone())
        }
        Type::AliasRef {
            mangled,
            package,
            name,
            args,
        } => {
            budget.charge(depth)?;
            let rehydrated_args: Vec<Type> = args
                .iter()
                .map(|a| rehydrate_reachable(a, types, skip, child, budget))
                .collect::<Result<_, _>>()?;
            let body = alias_ref_body(mangled, name, &rehydrated_args, types);
            budget.charge_copy(&body, child)?;
            if let Some(utility) = super::utility_types::utility(mangled) {
                // A deferred application stays a name; a concrete one is the
                // type it stands for, labeled as a written one is.
                return Ok(match body {
                    Type::AliasRef { .. } => body,
                    body => super::utility_types::label(utility, rehydrated_args, body),
                });
            }
            Ok(Type::alias_ty(
                package.clone(),
                name.clone(),
                mangled.clone(),
                rehydrated_args,
                Box::new(body),
            ))
        }
        // Carrying alias: rehydrate args, but leave the stored body —
        // its `AliasRef` leaves are the legitimate finite encoding.
        Type::Alias {
            mangled,
            package,
            name,
            args,
            ty: body,
        } => {
            budget.charge(depth)?;
            let rehydrated_args: Vec<Type> = args
                .iter()
                .map(|a| rehydrate_reachable(a, types, skip, child, budget))
                .collect::<Result<_, _>>()?;
            budget.charge_copy(body, child)?;
            Ok(Type::alias_ty(
                package.clone(),
                name.clone(),
                mangled.clone(),
                rehydrated_args,
                body.clone(),
            ))
        }
        _ => {
            budget.charge(depth)?;
            map_children(ty, |inner| {
                rehydrate_reachable(inner, types, skip, child, budget)
            })
        }
    }
}

/// Every member type of an interface or class: fields, properties, an index
/// signature's value, method, static and accessor signatures, and the
/// constructor's parameters.
fn for_each_member_type_mut(kind: &mut TypeKind, settle: &mut dyn FnMut(&mut Type)) {
    let method = |method: &mut crate::MethodSig, settle: &mut dyn FnMut(&mut Type)| {
        method
            .params
            .iter_mut()
            .for_each(|param| settle(&mut param.ty));
        settle(&mut method.ret);
    };
    match kind {
        TypeKind::Interface {
            methods,
            properties,
            index,
            ..
        } => {
            methods.values_mut().for_each(|sig| method(sig, settle));
            properties
                .values_mut()
                .for_each(|property| settle(&mut property.ty));
            if let Some(index) = index {
                settle(&mut index.value);
            }
        }
        TypeKind::Class {
            fields,
            methods,
            accessors,
            constructor,
            statics,
            static_fields,
            ..
        } => {
            fields
                .values_mut()
                .chain(static_fields.values_mut())
                .for_each(|field| settle(&mut field.ty));
            methods
                .values_mut()
                .chain(statics.values_mut())
                .for_each(|sig| method(sig, settle));
            for accessor in accessors {
                match accessor {
                    crate::AccessorSig::Getter { ret_ty, .. } => settle(ret_ty),
                    crate::AccessorSig::Setter { param, .. } => settle(&mut param.ty),
                }
            }
            constructor
                .iter_mut()
                .for_each(|param| settle(&mut param.ty));
        }
        _ => {}
    }
}
