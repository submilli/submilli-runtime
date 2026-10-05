//! Generic inference — body walks for generic and non-generic function decls,
//! call-site type-param unification, and the GP→TypeVar erasure pass.
//!
//! Invariant: `GenericParam`s are allocated in `infer_functions` and live only
//! inside `scopes`/`current_return` during the body walk. Downstream passes
//! (PackageDeclaration, codegen) see only `TypeVar`s.

use crate::compiler_error::CompilerFailure;

use std::collections::BTreeMap;

use crate::{
    ExprId, ExprKind, Ident, MethodSig, Span, StmtKind, Type, TypeAnnotation, TypedExprKind,
    TypedParam, TypedStmt, TypedStmtKind, ValueKind,
};

use super::{Inferer, type_limit_at};
use crate::type_size::{TypeBudget, TypeLimits, TypeTooLarge, map_children};
use crate::typechecker::type_param_substitution::{TypeParamSubstitution, UnifyError};

/// What a receiver-less generic call is calling. Only diagnostics differ: a
/// constructor has to be described as one, because `function Box<T>(…)` is not
/// a form anyone can write and `Box<string>(…)` without `new` is two more
/// errors.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum GenericCallee {
    Function,
    Constructor,
}

impl GenericCallee {
    /// How the diagnostic names the thing being called.
    fn subject(self, name: &str, generics: &[String]) -> String {
        match self {
            Self::Function => format!("`{name}`"),
            Self::Constructor if generics.is_empty() => format!("constructor of `{name}`"),
            Self::Constructor => {
                format!("constructor of `{name}<{}>`", generics.join(", "))
            }
        }
    }

    /// The "add an explicit type argument" hint, in a form that compiles: every
    /// parameter has to be listed, not just the ones inference failed on, or
    /// following the hint trades this error for an arity error.
    fn explicit_arg_hint(
        self,
        name: &str,
        generics: &[String],
        sub: &crate::typechecker::type_param_substitution::TypeParamSubstitution,
        limits: &TypeLimits,
    ) -> String {
        let args = generics
            .iter()
            .map(|g| {
                // Leave an uninferable parameter as its own name for the
                // caller to replace; show the ones inference did settle. One
                // too large to build is left for the caller too.
                match sub.apply(&Type::TypeVar(g.clone()), limits) {
                    Ok(Type::TypeVar(n)) if n == g[..] => g.clone(),
                    Ok(resolved) => resolved.to_string(),
                    Err(exceeded) => {
                        limits.record(exceeded);
                        g.clone()
                    }
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        match self {
            Self::Function => {
                format!("add an explicit type argument: `{name}<{args}>(…)`")
            }
            Self::Constructor => {
                format!("add an explicit type argument: `new {name}<{args}>(…)`")
            }
        }
    }
}

use super::format_signature::SignatureKind;
use crate::Param;

/// Replaces each `TypeVar` bound in `bindings` with its binding, once: unlike
/// [`TypeParamSubstitution::apply`] it does not chase variables inside a
/// binding. Fails, without building the rest, once the result passes a type
/// limit.
///
/// [`TypeParamSubstitution::apply`]: crate::typechecker::type_param_substitution::TypeParamSubstitution::apply
pub(super) fn substitute_typevars(
    ty: &Type,
    bindings: &BTreeMap<String, Type>,
    limits: &TypeLimits,
) -> Result<Type, TypeTooLarge> {
    substitute_at(ty, bindings, 1, &mut limits.budget())
}

/// [`substitute_typevars`] where the caller cannot return an error: a type past
/// a limit is recorded in `limits` and comes back as `Type::Error`.
pub(super) fn substitute_or_record(
    ty: &Type,
    bindings: &BTreeMap<String, Type>,
    limits: &TypeLimits,
) -> Type {
    limits.type_or_error(substitute_typevars(ty, bindings, limits))
}

fn substitute_at(
    ty: &Type,
    bindings: &BTreeMap<String, Type>,
    depth: u32,
    budget: &mut TypeBudget<'_>,
) -> Result<Type, TypeTooLarge> {
    if let Type::TypeVar(name) = ty
        && let Some(bound) = bindings.get(name)
    {
        budget.charge_copy(bound, depth)?;
        return Ok(bound.clone());
    }
    budget.charge(depth)?;
    let child = depth.saturating_add(1);
    map_children(ty, |inner| substitute_at(inner, bindings, child, budget))
}

pub(super) fn erase_generic_params(ty: &Type) -> Type {
    match ty {
        Type::Refined { original, ty } => Type::Refined {
            original: Box::new(erase_generic_params(original)),
            ty: Box::new(erase_generic_params(ty)),
        },
        Type::GenericParam { name, .. } => Type::TypeVar(name.clone()),
        Type::Object { fields, index } => Type::Object {
            index: index.as_ref().map(|i| i.map_value(erase_generic_params)),
            fields: fields
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        crate::ObjectField {
                            ty: erase_generic_params(&v.ty),
                            optional: v.optional,
                            readonly: v.readonly,
                        },
                    )
                })
                .collect(),
        },
        Type::Array(elem) => Type::Array(Box::new(erase_generic_params(elem))),
        Type::Readonly(inner) => Type::Readonly(Box::new(erase_generic_params(inner))),
        Type::Tuple(elements) => Type::Tuple(elements.iter().map(erase_generic_params).collect()),
        Type::Function {
            params,
            ret,
            predicate,
            has_rest,
        } => Type::Function {
            params: params.iter().map(erase_generic_params).collect(),
            ret: Box::new(erase_generic_params(ret)),
            predicate: predicate.as_ref().map(|p| {
                Box::new(crate::TypePredicate {
                    parameter_index: p.parameter_index,
                    asserted_type: erase_generic_params(&p.asserted_type),
                })
            }),
            has_rest: *has_rest,
        },
        Type::InterfaceRef {
            mangled,
            package,
            name,
            args,
        } => Type::InterfaceRef {
            mangled: mangled.clone(),
            package: package.clone(),
            name: name.clone(),
            args: args.iter().map(erase_generic_params).collect(),
        },
        Type::ClassRef {
            mangled,
            package,
            name,
            args,
        } => Type::ClassRef {
            mangled: mangled.clone(),
            package: package.clone(),
            name: name.clone(),
            args: args.iter().map(erase_generic_params).collect(),
        },
        Type::Union(members) => Type::union(members.iter().map(erase_generic_params).collect()),
        Type::Alias {
            mangled,
            package,
            name,
            args,
            ty: inner,
        } => Type::Alias {
            mangled: mangled.clone(),
            package: package.clone(),
            name: name.clone(),
            args: args.iter().map(erase_generic_params).collect(),
            ty: Box::new(erase_generic_params(inner)),
        },
        Type::AliasRef {
            mangled,
            package,
            name,
            args,
        } => Type::AliasRef {
            mangled: mangled.clone(),
            package: package.clone(),
            name: name.clone(),
            args: args.iter().map(erase_generic_params).collect(),
        },
        Type::TypeVar(_)
        | Type::Number
        | Type::BigInt
        | Type::NumberLiteral(_)
        | Type::String
        | Type::StringLiteral(_)
        | Type::Uint8Array
        | Type::Boolean
        | Type::BooleanLiteral(_)
        | Type::Null
        | Type::Void
        | Type::Never
        | Type::Unknown
        | Type::Error
        | Type::NumberEnum { .. }
        | Type::StringEnum { .. } => ty.clone(),
    }
}

/// Erase a typed expression's own type *and* every type embedded in its kind.
///
/// The node's `ty` is not the only spelling that escapes: an object literal's
/// field types become the key the shape collector and codegen agree on, a
/// closure's parameter types become its signature, and so on. Leaving those as
/// `GenericParam` while the node's `ty` says `TypeVar` produces two spellings
/// of one shape, and codegen then fails to find what it registered.
pub(super) fn erase_generic_params_in_expr(expr: &mut crate::TypedExpr) {
    expr.ty = erase_generic_params(&expr.ty);
    match &mut expr.kind {
        TypedExprKind::ObjectLiteral { fields, .. } => {
            for f in fields {
                f.ty = erase_generic_params(&f.ty);
            }
        }
        TypedExprKind::ArrayLiteral { element_ty, .. } => {
            *element_ty = erase_generic_params(element_ty);
        }
        TypedExprKind::TupleLiteral { element_types, .. } => {
            for t in element_types {
                *t = erase_generic_params(t);
            }
        }
        TypedExprKind::Closure {
            params,
            return_type,
            captured,
            ..
        } => {
            for p in params {
                p.ty = erase_generic_params(&p.ty);
            }
            *return_type = erase_generic_params(return_type);
            for c in captured {
                c.ty = erase_generic_params(&c.ty);
            }
        }
        TypedExprKind::GenericCall {
            type_args,
            return_cast,
            ..
        } => {
            for arg in type_args {
                *arg = erase_generic_params(arg);
            }
            if let Some(t) = return_cast {
                *t = erase_generic_params(t);
            }
        }
        TypedExprKind::GenericMethodCall {
            return_cast: Some(t),
            ..
        } => {
            *t = erase_generic_params(t);
        }
        TypedExprKind::InstanceOf { class, .. } => {
            *class = erase_generic_params(class);
        }
        TypedExprKind::Cast { target_ty, .. } => {
            *target_ty = erase_generic_params(target_ty);
        }
        _ => {}
    }
}

pub(super) fn erase_generic_params_in_stmt(stmt: &mut TypedStmt) {
    match &mut stmt.kind {
        TypedStmtKind::Let { ty, .. } | TypedStmtKind::Const { ty, .. } => {
            *ty = erase_generic_params(ty);
        }
        _ => {}
    }
}

/// True when a generic wrapper's declared return lowers to the universal
/// `(ref null $Object)` slot, so the substituted call result must be cast
/// back to its concrete shape at the call boundary. Covers a bare `T`, an
/// erased union (`T | null` and friends), and `unknown` — mirrors the
/// `needs_box` shape used for the non-generic-method return path.
fn return_erases_to_object_slot(ret: &Type) -> bool {
    matches!(
        ret.peel(),
        Type::TypeVar(_) | Type::Unknown | Type::Union(_)
    )
}

impl Inferer<'_> {
    fn resolve_call_type_argument(
        &mut self,
        annot: &TypeAnnotation,
        subject: &str,
        allow_void: bool,
    ) -> Result<Type, CompilerFailure> {
        let resolved = self.resolve_type(annot)?;
        if let Some(offender) =
            super::void_type_arguments::invalid_argument(&resolved, allow_void, self.resolver())
        {
            let offender = offender.clone();
            self.error(
                annot.span,
                format!(
                    "`{offender}` cannot be used as a type argument to {subject} — use a value type"
                ),
            );
            return Ok(Type::Error);
        }
        Ok(resolved)
    }

    fn type_parameter_allows_void(&self, name: &str, params: &[crate::Param], ret: &Type) -> bool {
        let sub = crate::typechecker::type_param_substitution::TypeParamSubstitution::from_pairs(
            &[name.to_string()],
            &[Type::Void],
        );
        let admits_void = |ty: &Type, return_position: bool| {
            let substituted = sub.apply_or_record(ty, &self.type_limits);
            super::void_type_arguments::invalid_position(
                &substituted,
                return_position,
                self.resolver(),
            )
            .is_none()
        };
        params.iter().all(|p| admits_void(&p.ty, false)) && admits_void(ret, true)
    }

    fn check_inferred_void_arguments(
        &mut self,
        params: &[crate::Param],
        ret: &Type,
        sub: &crate::typechecker::type_param_substitution::TypeParamSubstitution,
        span: Span,
    ) {
        let void_position = |ty: &Type, return_position: bool| {
            let substituted = sub.apply_or_record(ty, &self.type_limits);
            super::void_type_arguments::invalid_position(
                &substituted,
                return_position,
                self.resolver(),
            )
        };
        let invalid = params
            .iter()
            .find_map(|p| void_position(&p.ty, false))
            .or_else(|| void_position(ret, true));
        if let Some(position) = invalid {
            self.error(
                span,
                format!("type argument containing `void` requires {position} — use a value type"),
            );
        }
    }

    /// Structural retry for unification: when a direct param/arg unify fails
    /// and the param (or a union member of it) is an interface, bind its type
    /// params by unifying member signatures against the arg's own interface
    /// form. The structural counterpart of nominal arg unification — keyed on
    /// [`Type::interface_routing`], never on interface names.
    fn structural_member_unify(
        &mut self,
        sub: &mut crate::typechecker::type_param_substitution::TypeParamSubstitution,
        param_ty: &Type,
        arg_ty: &Type,
    ) -> bool {
        let members: Vec<Type> = match param_ty.peel() {
            Type::Union(ms) => ms.clone(),
            other => vec![other.clone()],
        };
        let Some((ma, _pa, na, aa)) = arg_ty.interface_routing() else {
            return false;
        };
        let Some(actual_form) = self.structural_form(&ma, na, &aa) else {
            return false;
        };
        for member in &members {
            // An object type routes to the `Object` interface, whose members
            // every value has; its own fields are unified directly.
            if matches!(member.peel(), Type::Object { .. }) {
                continue;
            }
            let Some((me, _pe, ne, ae)) = member.interface_routing() else {
                continue;
            };
            if me == ma {
                continue;
            }
            let Some(expected_form) = self.structural_form(&me, ne, &ae) else {
                continue;
            };
            if expected_form.is_empty() {
                continue;
            }
            let snapshot = sub.clone();
            let resolver = self.resolver();
            let all = expected_form.iter().all(|(name, exp)| {
                actual_form
                    .get(name)
                    .is_some_and(|act| sub.unify(&exp.ty, &act.ty, resolver).is_ok())
            });
            if all {
                return true;
            }
            *sub = snapshot;
        }
        false
    }
    pub(super) fn infer_functions(&mut self) -> Result<(), CompilerFailure> {
        let top_level: Vec<_> = self.ast.top_level.clone();
        for stmt_id in top_level {
            let stmt = self
                .ast
                .try_stmt(stmt_id)
                .map_err(super::arena_failure)?
                .clone();
            let span = stmt.span;
            let StmtKind::Function {
                name,
                params,
                body,
                doc,
                ..
            } = stmt.kind
            else {
                continue;
            };
            let (generic_names, sig_param_types, sig_ret_type, sig_param_defaults): (
                Vec<String>,
                Vec<Type>,
                Type,
                Vec<Option<crate::DefaultValue>>,
            ) = match self.top_symbols.get(&name.name) {
                Some(entry) => match &entry.kind {
                    ValueKind::Function {
                        generics,
                        params,
                        ret,
                        ..
                    } => (
                        generics.clone(),
                        params.iter().map(|p| p.ty.clone()).collect(),
                        ret.clone(),
                        params.iter().map(|p| p.default.clone()).collect(),
                    ),
                    _ => (
                        Vec::new(),
                        vec![Type::Error; params.len()],
                        Type::Error,
                        vec![None; params.len()],
                    ),
                },
                None => (
                    Vec::new(),
                    vec![Type::Error; params.len()],
                    Type::Error,
                    vec![None; params.len()],
                ),
            };
            let resolved_predicate = match self.top_symbols.get(&name.name) {
                Some(entry) => match &entry.kind {
                    ValueKind::Function { type_predicate, .. } => type_predicate.clone(),
                    _ => None,
                },
                None => None,
            };
            let body_instantiation = self.push_body_generics(generic_names.clone())?;
            let body_param_types: Vec<Type> = sig_param_types
                .iter()
                .map(|t| substitute_typevars(t, &body_instantiation, &self.type_limits))
                .collect::<Result<_, _>>()
                .map_err(type_limit_at(name.span))?;
            let body_ret_type =
                substitute_typevars(&sig_ret_type, &body_instantiation, &self.type_limits)
                    .map_err(type_limit_at(name.span))?;
            // typed_params stores signature TypeVars, not body GPs — GPs must
            // not escape the body; PackageDeclaration and codegen read these externally.
            let typed_params: Vec<TypedParam> = params
                .iter()
                .zip(sig_param_types.iter())
                .zip(sig_param_defaults.iter())
                .map(|((p, ty), default)| TypedParam {
                    name: p.name.clone(),
                    ty: ty.clone(),
                    boxed: false,
                    rest: p.rest,
                    default: default.clone(),
                })
                .collect();
            let stored_return = sig_ret_type.clone();
            self.scopes.push();
            for (p, body_ty) in params.iter().zip(body_param_types.iter()) {
                self.scopes.insert_annotated_param(
                    p.name.name.clone(),
                    body_ty.clone(),
                    p.name.span,
                );
            }
            let prev_return = self.current_return.replace(body_ret_type.clone());
            // Reset to `true`: a previous function that ended unreachable would
            // otherwise taint this body's reachability joins, dropping post-if
            // narrowings.
            let prev_reachable = std::mem::replace(&mut self.reachable, true);
            let prev_predicate = self.current_type_predicate.take();
            if let Some(pred) = &resolved_predicate {
                let idx = pred.parameter_index as usize;
                if let Some(param) = params.get(idx) {
                    self.current_type_predicate = Some((pred.clone(), param.name.name.clone()));
                }
            }
            // Snapshot arena lengths to bound which IDs this body walk allocates —
            // only those need GP erasure.
            let exprs_before = self.typed_ast.exprs_len();
            let stmts_before = self.typed_ast.stmts_len();
            let body_id = self
                .infer_body_with_narrowing_boundary(body)?
                .ok_or_else(|| {
                    super::inference_failure("function body is a Block, never a type-only decl")
                })?;
            self.current_return = prev_return;
            self.current_type_predicate = prev_predicate;
            self.reachable = prev_reachable;
            self.scopes.pop();
            self.pop_body_generics();
            for id in self
                .typed_ast
                .expr_ids()
                .map_err(crate::typechecker::arena_failure)?
                .skip(exprs_before)
            {
                erase_generic_params_in_expr(
                    self.typed_ast
                        .try_expr_mut(id)
                        .map_err(crate::typechecker::arena_failure)?,
                );
            }
            for id in self
                .typed_ast
                .stmt_ids()
                .map_err(crate::typechecker::arena_failure)?
                .skip(stmts_before)
            {
                erase_generic_params_in_stmt(
                    self.typed_ast
                        .try_stmt_mut(id)
                        .map_err(crate::typechecker::arena_failure)?,
                );
            }
            let ret_type = stored_return;
            let mangled_name = self.mangle_top_symbol(&name.name)?;
            self.add_typed_function(crate::TypedFunction {
                name,
                mangled_name,
                generics: generic_names,
                params: typed_params,
                return_type: ret_type,
                type_predicate: resolved_predicate,
                body: body_id,
                doc,
                span,
            })?;
        }

        Ok(())
    }

    /// Generic interface-method dispatch. Pre-seeds the unification map with the
    /// receiver's interface bindings so interface-level and method-level generics
    /// share one map — method-level unification won't accidentally rebind them.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn infer_generic_method_call(
        &mut self,
        typed_receiver: ExprId,
        iface_mangled: crate::MangledName,
        name: Ident,
        sig: MethodSig,
        interface_bindings: BTreeMap<String, Type>,
        type_args: Option<Vec<TypeAnnotation>>,
        args: Vec<ExprId>,
        expected: Option<&Type>,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        // The no-mapper form preserves the source type; the mapped form has
        // an independent result parameter, like TypeScript's two overloads.
        let mut sig = sig;
        if iface_mangled == crate::mangle::prelude("ArrayConstructor")
            && name.name == "from"
            && (args.len() == 1
                || match args.get(1) {
                    Some(id) => matches!(
                        self.ast.try_expr(*id).map_err(super::arena_failure)?.kind,
                        crate::ExprKind::Null
                    ),
                    None => false,
                })
        {
            sig.generics = vec!["T".into()];
            sig.ret = Type::Array(Box::new(Type::TypeVar("T".into())));
            sig.params
                .get_mut(1)
                .ok_or_else(|| super::inference_failure("missing Array.from map parameter"))?
                .ty = Type::Null;
        }

        let receiver_ty = self
            .typed_ast
            .try_expr(typed_receiver)
            .map_err(crate::typechecker::arena_failure)?
            .ty
            .clone();
        let mut sub = TypeParamSubstitution::new();

        for (k, v) in interface_bindings {
            sub.insert(k, v);
        }

        if let Some(targs) = &type_args {
            if targs.len() != sig.generics.len() {
                let help = self.format_signature(SignatureKind::Method {
                    receiver_ty: &receiver_ty,
                    name: &name.name,
                    sig: &sig,
                });
                self.error_with_help(
                    span,
                    format!(
                        "method `{}` expects {} type argument(s), got {}",
                        name.name,
                        sig.generics.len(),
                        targs.len(),
                    ),
                    vec![help],
                );
            }
            let subject = format!("method `{}`", name.name);
            for (gname, annot) in sig.generics.iter().zip(targs.iter()) {
                let resolved = self.resolve_call_type_argument(
                    annot,
                    &subject,
                    self.type_parameter_allows_void(gname, &sig.params, &sig.ret),
                )?;
                sub.insert(gname.clone(), resolved);
            }
        }

        if let Some(want) = expected.filter(|want| pins_type_parameters(want)) {
            self.bind_from_expected_result(&mut sub, &sig.ret, want);
        }

        let has_rest = sig.params.last().is_some_and(|p| p.rest);
        let fixed_count = sig.params.iter().take_while(|p| !p.rest).count();
        let max_args = if has_rest {
            usize::MAX
        } else {
            sig.params.len()
        };
        let min_args = sig
            .params
            .iter()
            .take_while(|p| !p.rest && p.default.is_none())
            .count();
        let arity_ok = args.len() >= min_args && args.len() <= max_args;
        if !arity_ok {
            let help = self.format_signature(SignatureKind::Method {
                receiver_ty: &receiver_ty,
                name: &name.name,
                sig: &sig,
            });
            let msg = if has_rest {
                format!(
                    "method `{}` expects {}+ argument(s), got {}",
                    name.name,
                    min_args,
                    args.len(),
                )
            } else if min_args == max_args {
                format!(
                    "method `{}` expects {} argument(s), got {}",
                    name.name,
                    max_args,
                    args.len(),
                )
            } else {
                format!(
                    "method `{}` expects {}-{} argument(s), got {}",
                    name.name,
                    min_args,
                    max_args,
                    args.len(),
                )
            };
            self.error_with_help(span, msg, vec![help]);
        }

        let rest_elem_ty: Type = if has_rest {
            match &sig
                .params
                .last()
                .ok_or_else(|| super::inference_failure("rest signature has no parameters"))?
                .ty
            {
                Type::Array(elem) => (**elem).clone(),
                _ => Type::Error,
            }
        } else {
            Type::Error
        };

        let signature_help = |this: &mut Self| {
            this.format_signature(SignatureKind::Method {
                receiver_ty: &receiver_ty,
                name: &name.name,
                sig: &sig,
            })
        };
        let errors_before_args = self.error_count();
        let inferred_generics: &[String] = if type_args.is_some() {
            &[]
        } else {
            &sig.generics
        };
        let mut typed_args = self.infer_generic_arguments(
            &args,
            &sig.params,
            &sig.ret,
            inferred_generics,
            &rest_elem_ty,
            &mut sub,
            signature_help,
        )?;

        if has_rest || typed_args.len() < sig.params.len() {
            self.typed_ast
                .record_authored_arguments(span, typed_args.clone());
        }
        if arity_ok {
            self.fill_omitted_defaults(&sig.params, args.len(), span, &mut typed_args)?;
        }
        if arity_ok && has_rest {
            self.pack_rest_tail(fixed_count, rest_elem_ty.clone(), span, &mut typed_args)?;
        }

        let array_from =
            iface_mangled == crate::mangle::prelude("ArrayConstructor") && name.name == "from";
        let array_from_mapper = array_from && sig.generics.len() == 2;
        let mapper_type = typed_args
            .get(1)
            .map(|id| {
                Ok::<_, crate::compiler_error::CompilerFailure>(
                    self.typed_ast
                        .try_expr(*id)
                        .map_err(crate::typechecker::arena_failure)?
                        .ty
                        .clone(),
                )
            })
            .transpose()?;
        // An array-like `{ length }` has no elements to infer the element type
        // from.
        if array_from
            && let Some(element) = sig.generics.first()
            && sub.get(element).is_none()
        {
            sub.insert(element.clone(), Type::Unknown);
        }
        if array_from_mapper
            && mapper_type
                .as_ref()
                .is_some_and(|ty| ty.peel() == &Type::Null)
        {
            let element = sub
                .apply(&Type::TypeVar("T".into()), &self.type_limits)
                .map_err(type_limit_at(span))?;
            sub.insert("U".into(), element);
        }

        sub.bind_whole_union_fallbacks();
        self.check_close_matches(&mut sub, span);
        self.bind_leftover_type_parameters(
            &mut sub,
            &sig.generics,
            &sig.ret,
            expected,
            errors_before_args,
        );
        self.bind_uninferred_to_unknown(&mut sub, &sig.generics, &iface_mangled, span)?;
        if let Err(unbound) = sub
            .resolve_all(&sig.generics, &self.type_limits)
            .map_err(type_limit_at(span))?
        {
            self.error_with_help(
                span,
                format!(
                    "cannot infer type parameter(s) {} for method `{}`; consider adding an explicit type argument",
                    unbound
                        .iter()
                        .map(|n| format!("`{n}`"))
                        .collect::<Vec<_>>()
                        .join(", "),
                    name.name,
                ),
                vec![format!(
                    "add an explicit type argument: `receiver.{}<T>(…)`",
                    name.name
                )],
            );
        }

        self.check_inferred_void_arguments(&sig.params, &sig.ret, &sub, span);
        let mut result_ty = self.instantiate(&sub, &sig.ret, span)?;
        if array_from_mapper && mapper_type.as_ref().is_some_and(|ty| {
            matches!(ty.peel(), Type::Union(members) if members.iter().any(|member| member.peel() == &Type::Null))
        }) {
            result_ty = Type::Array(Box::new(Type::union(vec![
                self.instantiate(&sub, &Type::TypeVar("T".into()), span)?,
                self.instantiate(&sub, &Type::TypeVar("U".into()), span)?,
            ])));
        }
        let type_predicate = match &sig.predicate {
            Some(p) => Some(Box::new(crate::TypePredicate {
                parameter_index: p.parameter_index,
                asserted_type: self.instantiate(&sub, &p.asserted_type, span)?,
            })),
            None => None,
        };
        self.record_generic_call_arguments(&typed_args, &sig.params, &sig.ret, type_args.is_some());

        // TypeVar params/return need box/cast at the Wasm boundary; composite types use plain MethodCall.
        let any_generic_arg = sig.params.iter().any(|p| matches!(p.ty, Type::TypeVar(_)));
        // The wrapper erases any TypeVar-mentioning return to `$Object`, so the
        // substituted result needs a cast back to its static shape — not only a
        // bare `V`, but also `V | null` and other erased unions (their object
        // members lower to the narrower `$ObjectShape`).
        let return_needs_cast = return_erases_to_object_slot(&sig.ret);
        if any_generic_arg || return_needs_cast {
            let generic_args: Vec<crate::GenericArgument> = typed_args
                .into_iter()
                .zip(sig.params.iter())
                .map(|(expr, p)| crate::GenericArgument {
                    expr,
                    is_generic: matches!(p.ty, Type::TypeVar(_)),
                })
                .collect();
            let return_cast = if return_needs_cast {
                Some(result_ty.clone())
            } else {
                None
            };
            return Ok((
                TypedExprKind::GenericMethodCall {
                    receiver: typed_receiver,
                    iface: iface_mangled,
                    name,
                    args: generic_args,
                    return_cast,
                    type_predicate,
                },
                result_ty,
            ));
        }
        // sub carries interface bindings even when the method itself isn't generic.
        Ok((
            TypedExprKind::MethodCall {
                receiver: typed_receiver,
                iface: iface_mangled,
                name,
                args: typed_args,
                type_predicate,
            },
            result_ty,
        ))
    }

    /// The lifted signature a generic-call diagnostic shows: a real function
    /// signature, or a class header with its constructor line.
    fn generic_callee_lift(
        &self,
        callee: GenericCallee,
        callee_ident: &Ident,
        generics: &[String],
        params: &[Param],
        ret: &Type,
    ) -> String {
        match callee {
            GenericCallee::Function => {
                let doc = self.lookup_function_doc(&callee_ident.name);
                self.format_signature(SignatureKind::Function {
                    name: &callee_ident.name,
                    predicate: None,
                    generics,
                    params,
                    ret,
                    doc: doc.as_ref(),
                })
            }
            GenericCallee::Constructor => {
                let header = if generics.is_empty() {
                    callee_ident.name.clone()
                } else {
                    format!("{}<{}>", callee_ident.name, generics.join(", "))
                };
                let rendered: Vec<String> = params
                    .iter()
                    .map(|p| format!("{}: {}", p.name, p.ty))
                    .collect();
                format!("class {header} {{ constructor({}); }}", rendered.join(", "))
            }
        }
    }

    /// Bind the type parameters the arguments left open: from an `unknown`
    /// expected result, which [`pins_type_parameters`] kept out of the
    /// bindings made before the arguments; then, if an argument was reported
    /// wrong (it may be what would have bound one), to `Error`, so the
    /// parameter isn't reported as uninferable too.
    fn bind_leftover_type_parameters(
        &self,
        sub: &mut TypeParamSubstitution,
        generics: &[String],
        ret: &Type,
        expected: Option<&Type>,
        errors_before_args: usize,
    ) {
        if let Some(want) = expected.filter(|want| !pins_type_parameters(want)) {
            let _ = sub.unify(ret, want, self.resolver());
        }
        if self.error_count() > errors_before_args
            && let Err(exceeded) = bind_remaining(sub, generics, Type::Error, &self.type_limits)
        {
            self.type_limits.record(exceeded);
        }
    }

    /// Bind type parameters from the type a call's result is expected to have,
    /// before its arguments are inferred, as bindings an argument may still
    /// replace (see [`TypeParamSubstitution::mark_from_expected_result`]).
    fn bind_from_expected_result(&self, sub: &mut TypeParamSubstitution, ret: &Type, want: &Type) {
        let before = sub.clone();
        self.bind_from_expected_type(sub, ret, want);
        sub.mark_from_expected_result(&before);
    }

    /// Bind type parameters by unifying the result type `ret` with `want`.
    ///
    /// When `want` is a union and the result type is neither a type
    /// variable nor a union (both unify with the whole union), the result is
    /// unified with each member of its own [`ShapeKind`]. The type parameters
    /// bind only if every such member unifies, all to the same bindings;
    /// otherwise nothing binds. `Cmp<T>` expected as `Cmp<P> | null` binds
    /// `T = P`.
    ///
    /// Adopting a partial match would fix the type parameters before the
    /// arguments are seen, although the result could still be assigned to a
    /// member unification can't match, such as a wider object or an interface.
    /// A mismatch is reported later, where the call's result is checked
    /// against the expected type.
    fn bind_from_expected_type(&self, sub: &mut TypeParamSubstitution, ret: &Type, want: &Type) {
        let unifies_with_whole_union = matches!(ret.peel(), Type::TypeVar(_) | Type::Union(_));
        let members = match want.peel() {
            Type::Union(members) if !unifies_with_whole_union => members,
            _ => {
                let _ = sub.unify(ret, want, self.resolver());
                return;
            }
        };
        let ret_kind = ShapeKind::of(ret);
        let mut agreed: Option<TypeParamSubstitution> = None;
        for member in members {
            if !ret_kind.could_be(ShapeKind::of(member)) {
                continue;
            }
            let mut trial = sub.clone();
            if trial.unify(ret, member, self.resolver()).is_err() {
                return;
            }
            match &agreed {
                Some(bindings) if *bindings != trial => return,
                _ => agreed = Some(trial),
            }
        }
        if let Some(bindings) = agreed {
            *sub = bindings;
        }
    }

    /// Bind the type parameters of a program's own generic that nothing
    /// inferred to `unknown`, as tsc does: every type is assignable to it.
    /// Built-in generics keep reporting them, since tsc gives several of them
    /// `any` instead (`new Map()` is `Map<any, any>`), which a later use may
    /// still resolve here (`new Map().set(k, v)`).
    fn bind_uninferred_to_unknown(
        &self,
        sub: &mut TypeParamSubstitution,
        generics: &[String],
        declared_by: &crate::MangledName,
        span: Span,
    ) -> Result<(), CompilerFailure> {
        if crate::mangle::is_builtin(declared_by) {
            return Ok(());
        }
        bind_remaining(sub, generics, Type::Unknown, &self.type_limits).map_err(type_limit_at(span))
    }

    /// Report each union argument member that closely matched a parameter
    /// member while inference ran and fits no member of the parameter now
    /// that it is done, against the member it closely matched.
    fn check_close_matches(&mut self, sub: &mut TypeParamSubstitution, span: Span) {
        for close_match in sub.take_close_matches() {
            if sub
                .clone()
                .unify(&close_match.param, &close_match.arg, self.resolver())
                .is_ok()
            {
                continue;
            }
            let (expected, got) =
                match sub
                    .clone()
                    .unify(&close_match.sibling, &close_match.arg, self.resolver())
                {
                    Err(UnifyError::Mismatch { expected, got }) => (expected, got),
                    _ => (close_match.sibling, close_match.arg),
                };
            self.error_with_help(
                span,
                format!("expected `{expected}`, got `{got}`"),
                super::type_diff::type_mismatch_help(&expected, &got),
            );
        }
    }

    /// `ty` with `sub` applied, at a call site at `span`.
    fn instantiate(
        &self,
        sub: &TypeParamSubstitution,
        ty: &Type,
        span: Span,
    ) -> Result<Type, CompilerFailure> {
        sub.apply(ty, &self.type_limits)
            .map_err(type_limit_at(span))
    }

    /// Bind type parameters from the annotated parameters of a function
    /// literal whose inference is deferred, so the arguments inferred before
    /// it see them: `reduce((acc: number[], x) => …, [])` types `[]` as
    /// `number[]`. The annotations are resolved again, and any error in them
    /// reported, when the literal itself is inferred.
    fn bind_from_annotated_params(
        &mut self,
        literal: ExprId,
        param_ty: &Type,
        sub: &mut TypeParamSubstitution,
    ) -> Result<(), CompilerFailure> {
        let Some(Type::Function { params, .. }) = function_part(param_ty) else {
            return Ok(());
        };
        let Some(declared_params) = self.function_literal_params(literal)? else {
            return Ok(());
        };
        let diagnostics_before = self.diagnostics.len();
        for (declared, param) in declared_params.iter().zip(params) {
            if let Some(annotation) = &declared.ty {
                let annotated = self.resolve_type(annotation)?;
                let _ = sub.unify_argument(param, &annotated, self.resolver());
            }
        }
        self.diagnostics.truncate(diagnostics_before);

        Ok(())
    }

    /// Start inferring an object literal argument's fields one at a time, when
    /// one of them is a function literal with an unannotated parameter: as in
    /// tsc, a type parameter an earlier field binds then types that function's
    /// parameters. `test({ produce: (n: number) => n, consume: (x) => ... })`
    /// types `x` from `produce`. Returns the inference this one interrupts.
    fn start_object_argument_inference(
        &mut self,
        arg: ExprId,
        param_ty: &Type,
        sub: &TypeParamSubstitution,
    ) -> Result<Option<ObjectArgumentInference>, CompilerFailure> {
        let enclosing = self.object_argument_inference.take();
        let ExprKind::ObjectLiteral { members } = self
            .ast
            .try_expr(arg)
            .map_err(super::arena_failure)?
            .kind
            .clone()
        else {
            return Ok(enclosing);
        };
        let mut has_context_sensitive_field = false;
        for member in &members {
            if let crate::ObjectLiteralMember::Field(field) = member {
                has_context_sensitive_field |= self.is_context_sensitive_function(field.value)?;
            }
        }
        let fields = match param_ty.peel() {
            Type::Object { fields, .. } => Some(fields.clone()),
            interface @ Type::InterfaceRef { .. } => {
                match super::assignable::expand_interface_data_shape(interface, self.resolver()) {
                    Some(Type::Object { fields, .. }) => Some(fields),
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some(fields) = fields.filter(|_| has_context_sensitive_field) {
            self.object_argument_inference = Some(ObjectArgumentInference {
                literal: arg,
                fields,
                sub: sub.clone(),
                fallback_echoes: Vec::new(),
            });
        }
        Ok(enclosing)
    }

    /// Keep what the fields of the argument bound, and resume `enclosing`.
    fn finish_object_argument_inference(
        &mut self,
        enclosing: Option<ObjectArgumentInference>,
        sub: &mut TypeParamSubstitution,
    ) {
        if let Some(mut finished) =
            std::mem::replace(&mut self.object_argument_inference, enclosing)
        {
            finished
                .sub
                .bind_whole_union_fallbacks_named(&finished.fallback_echoes);
            *sub = finished.sub;
        }
    }

    /// The hint for field `name` of `literal`, with what its earlier fields
    /// bound, if the literal's fields are being inferred one at a time. A
    /// callback field's parameters take the whole-union fallbacks of the type
    /// parameters still unbound, as [`fix_callback_parameters`] gives them;
    /// they apply to the hint only, so its returns or a later field can still
    /// bind them.
    pub(super) fn object_argument_field_hint(&self, literal: ExprId, name: &str) -> Option<Type> {
        let inference = self
            .object_argument_inference
            .as_ref()
            .filter(|inference| inference.literal == literal)?;
        let field = inference.fields.get(name)?;
        let mut sub = inference.sub.clone();
        if let Some(Type::Function { params, .. }) = function_part(&field.ty) {
            sub.bind_whole_union_fallbacks_in(params);
        }
        Some(sub.apply_or_record(&field.ty, &self.type_limits))
    }

    /// Bind the type parameters field `name` of `literal` determines, for the
    /// fields after it. A mismatch is reported when the whole argument is.
    pub(super) fn infer_from_object_argument_field(
        &mut self,
        literal: ExprId,
        name: &str,
        value_ty: &Type,
    ) {
        let Some(mut inference) = self
            .object_argument_inference
            .take_if(|inference| inference.literal == literal)
        else {
            return;
        };
        if let Some(field) = inference.fields.get(name) {
            let before = inference.sub.clone();
            let _ = inference
                .sub
                .unify_argument(&field.ty, value_ty, self.resolver());
            let echoes = inference.sub.unbind_fallback_echoes(&before);
            inference.fallback_echoes.extend(echoes);
        }
        self.object_argument_inference = Some(inference);
    }

    fn function_literal_params(
        &self,
        expr: ExprId,
    ) -> Result<Option<Vec<crate::ParamDecl>>, CompilerFailure> {
        Ok(
            match &self.ast.try_expr(expr).map_err(super::arena_failure)?.kind {
                ExprKind::Paren(inner) => self.function_literal_params(*inner)?,
                ExprKind::FunctionExpression { function, .. } => {
                    self.function_literal_params(*function)?
                }
                ExprKind::Arrow { params, .. } => Some(params.clone()),
                _ => None,
            },
        )
    }

    /// A function literal with a parameter left for its context to type,
    /// which is what TypeScript infers after the other arguments.
    fn is_context_sensitive_function(&self, expr: ExprId) -> Result<bool, CompilerFailure> {
        Ok(self
            .function_literal_params(expr)?
            .is_some_and(|params| params.iter().any(|p| p.ty.is_none())))
    }

    /// Infer a generic call's arguments against `params`, binding its type
    /// parameters in `sub`. A function literal with an unannotated parameter,
    /// passed for a function-typed parameter, is inferred after the others,
    /// so its hint sees what they bound: `reduce((acc, x) => acc + x, 0)` binds
    /// `U` from `0` before typing `acc`. Only such a literal is deferred:
    /// creating it runs nothing and its body sees no outer narrowing, so
    /// checking it late can't observe a later argument's assignment, while any
    /// other argument could. A fully annotated one binds from its annotations
    /// in order, as in TypeScript.
    ///
    /// `inferred_generics` names the type parameters the call infers rather
    /// than takes as written type arguments. An object literal one of them
    /// types, when only literals are its candidates, is a source for it and
    /// isn't checked for unknown fields ([`super::inference_sources`]).
    ///
    /// `signature_help` renders the callee for a mismatch diagnostic.
    #[allow(clippy::too_many_arguments)]
    fn infer_generic_arguments(
        &mut self,
        args: &[ExprId],
        params: &[Param],
        ret: &Type,
        inferred_generics: &[String],
        rest_elem_ty: &Type,
        sub: &mut TypeParamSubstitution,
        signature_help: impl Fn(&mut Self) -> String,
    ) -> Result<Vec<ExprId>, CompilerFailure> {
        let has_rest = params.last().is_some_and(|p| p.rest);
        let fixed_count = params.iter().take_while(|p| !p.rest).count();
        let args_with_param_types: Vec<(ExprId, Type)> = args
            .iter()
            .enumerate()
            .map(|(i, &arg_id)| {
                let param_ty = match params.get(i) {
                    Some(param) if i < fixed_count => param.ty.clone(),
                    _ if has_rest => rest_elem_ty.clone(),
                    _ => Type::Error,
                };
                (arg_id, param_ty)
            })
            .collect();
        let arguments = GenericArguments {
            inferred_generics,
            sourced_by_literals: self
                .literal_inferred_type_params(&args_with_param_types, inferred_generics)?,
            literal_types: LiteralTypeArguments::new(
                &args_with_param_types,
                ret,
                inferred_generics,
            ),
        };
        let mut typed_slots: Vec<Option<ExprId>> = vec![None; args.len()];
        for deferred_pass in [false, true] {
            for (i, (arg_id, param_ty)) in args_with_param_types.iter().enumerate() {
                let (arg_id, param_ty) = (*arg_id, param_ty.clone());
                if typed_slots[i].is_some() {
                    continue;
                }
                let deferred = function_part(&param_ty).is_some()
                    && self.is_context_sensitive_function(arg_id)?;
                if deferred && !deferred_pass {
                    self.bind_from_annotated_params(arg_id, &param_ty, sub)?;
                }
                if deferred != deferred_pass {
                    continue;
                }
                if deferred {
                    fix_callback_parameters(sub, &param_ty, inferred_generics);
                }
                let errors_before = self.error_count();
                let (typed_id, arg_ty) =
                    self.infer_generic_argument(arg_id, &param_ty, &arguments, sub)?;
                typed_slots[i] = Some(typed_id);
                let missing_slot = i >= fixed_count && !has_rest;
                if missing_slot || matches!(arg_ty, Type::Error) {
                    continue;
                }
                // An error inferring the argument already covers a mismatch here.
                let already_reported = self.error_count() > errors_before;
                // The argument was already reported against the expected
                // result's binding; replacing it would report the result too.
                if already_reported {
                    sub.keep_replaceable_bindings(&param_ty);
                }
                if let Err(error) = sub.unify_argument(&param_ty, &arg_ty, self.resolver()) {
                    self.unify_argument_error(
                        error,
                        sub,
                        (&param_ty, &arg_ty),
                        typed_id,
                        already_reported,
                        &signature_help,
                    )?;
                }
            }
        }
        Ok(typed_slots.into_iter().flatten().collect())
    }

    /// Infer one argument of a generic call against `param_ty`, with what
    /// `sub` has bound so far as its hint, and return the type to unify with
    /// `param_ty`: a fresh literal widened where tsc would widen it.
    fn infer_generic_argument(
        &mut self,
        arg_id: ExprId,
        param_ty: &Type,
        arguments: &GenericArguments,
        sub: &mut TypeParamSubstitution,
    ) -> Result<(ExprId, Type), CompilerFailure> {
        // An oversized hint fails at the argument's own checkpoint.
        let hint = sub.apply_or_record(param_ty, &self.type_limits);
        let hint = self.literal_argument_hint(arg_id, hint, arguments.inferred_generics)?;
        let hinted_by_replaceable_binding = sub.mentions_replaceable_binding(param_ty);
        if hinted_by_replaceable_binding {
            self.arguments_with_replaceable_hints.insert(arg_id);
        }
        let enclosing = self.start_object_argument_inference(arg_id, param_ty, sub)?;
        let keeps_literal = arguments.literal_types.keeps(param_ty);
        let inferred = self.with_inferred_positions(
            arg_id,
            param_ty,
            &arguments.sourced_by_literals,
            |this| {
                this.keeps_literal_types = keeps_literal;
                this.next_function_keeps_returned_literals = keeps_literal;
                this.infer_expr(arg_id, Some(&hint))
            },
        );
        self.finish_object_argument_inference(enclosing, sub);
        self.arguments_with_replaceable_hints.remove(&arg_id);
        let (typed_id, arg_ty) = inferred?;
        if keeps_literal {
            self.record_kept_literal_argument(typed_id);
        }
        // A literal that fits what the type parameter is already bound to is
        // not widened, so it doesn't conflict with that binding:
        // `pick(mode, "off")` with `mode: Mode` binds `Mode`, as tsc does when
        // the type parameter is the result (where it isn't, tsc widens to
        // `string`, and a later push of another string is SUB-1397). Nor is
        // one the call's expected result asks for: `const f: () => "a" =
        // later(c)` binds `"a"`.
        let hint_is_known = !super::expr::mentions_type_var(&hint, &|var| {
            arguments.inferred_generics.iter().any(|name| name == var)
        });
        let fits_binding = (hinted_by_replaceable_binding || hint_is_known)
            && super::assignable(&arg_ty, &hint, self.resolver());
        if arguments.literal_types.widens(param_ty) && !fits_binding {
            let widened = self.widen_fresh_literals(typed_id, &arg_ty)?;
            return Ok((typed_id, widened));
        }
        Ok((typed_id, arg_ty))
    }

    /// The hint for an object or array literal argument, with each data-only
    /// interface whose type arguments are still being inferred replaced by
    /// its fields, so the literal keeps its own field types to bind them
    /// from. Typed as `Box<T>` itself, it would bind nothing
    /// (`unbox({ v: "s" })`).
    fn literal_argument_hint(
        &self,
        arg_id: ExprId,
        hint: Type,
        inferred_generics: &[String],
    ) -> Result<Type, CompilerFailure> {
        if !self.builds_literal(arg_id)? {
            return Ok(hint);
        }
        Ok(expand_hint_interfaces(
            &hint,
            inferred_generics,
            self.resolver(),
        ))
    }

    /// Whether `expr` is an object or array literal, in parentheses or as both
    /// of a conditional's branches.
    fn builds_literal(&self, expr: ExprId) -> Result<bool, CompilerFailure> {
        Ok(
            match &self.ast.try_expr(expr).map_err(super::arena_failure)?.kind {
                ExprKind::ObjectLiteral { .. } | ExprKind::ArrayLiteral { .. } => true,
                ExprKind::Paren(inner) => self.builds_literal(*inner)?,
                ExprKind::Ternary { then_, else_, .. } => {
                    self.builds_literal(*then_)? && self.builds_literal(*else_)?
                }
                _ => false,
            },
        )
    }

    /// Handle an argument that didn't unify with its parameter: a structural
    /// match may still bind it; otherwise report it, unless `already_reported`.
    fn unify_argument_error(
        &mut self,
        error: UnifyError,
        sub: &mut TypeParamSubstitution,
        (param_ty, arg_ty): (&Type, &Type),
        arg: ExprId,
        already_reported: bool,
        signature_help: &impl Fn(&mut Self) -> String,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        let arg_span = self
            .typed_ast
            .try_expr(arg)
            .map_err(crate::typechecker::arena_failure)?
            .span;
        let _: () = match error {
            UnifyError::Conflict { .. } if already_reported => {}
            UnifyError::Conflict { name, prev, new } => self.error(
                arg_span,
                format!(
                    "type parameter `{name}` already bound to `{prev}`, cannot bind to `{new}`"
                ),
            ),
            UnifyError::Mismatch { expected, got } => {
                if self.structural_member_unify(sub, param_ty, arg_ty) || already_reported {
                    return Ok(());
                }
                let mut help = vec![signature_help(self)];
                help.extend(super::type_diff::type_mismatch_help(&expected, &got));
                self.error_with_help(
                    arg_span,
                    format!("expected `{expected}`, got `{got}`"),
                    help,
                );
            }
        };
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn infer_generic_call(
        &mut self,
        callee_ident: Ident,
        generics: Vec<String>,
        params: Vec<Param>,
        ret: Type,
        mangled: crate::MangledName,
        predicate: Option<crate::TypePredicate>,
        type_args: Option<Vec<crate::TypeAnnotation>>,
        args: Vec<ExprId>,
        expected: Option<&Type>,
        span: Span,
        callee: GenericCallee,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        let mut sub = TypeParamSubstitution::new();
        let type_args_written = type_args.is_some();
        // `session.get<T>` is lowered as a runtime-checked cast rather than an
        // ordinary generic call; the two sites below are the two halves of that
        // one decision, so it is read once here.
        let checked_get = crate::stdlib::session::declaration::is_checked_get(&mangled);
        // `llm.call<T>` / `llm.batch<T>` lower the same way and for the same
        // reason. They differ from `session.get` only in what a bare call means
        // — the `Completion` envelope rather than an unchecked `unknown` — and
        // in carrying a compile-time schema into the trailing argument.
        let checked_llm = crate::stdlib::llm::declaration::is_checked_call(&mangled);

        if let Some(targs) = &type_args {
            if targs.len() != generics.len() {
                let help =
                    self.generic_callee_lift(callee, &callee_ident, &generics, &params, &ret);
                self.error_with_help(
                    span,
                    format!(
                        "{} expects {} type argument(s), got {}",
                        callee.subject(&callee_ident.name, &generics),
                        generics.len(),
                        targs.len(),
                    ),
                    vec![help],
                );
            }
            let subject = callee.subject(&callee_ident.name, &generics);
            for (name, annot) in generics.iter().zip(targs.iter()) {
                let resolved = self.resolve_call_type_argument(
                    annot,
                    &subject,
                    callee == GenericCallee::Function
                        && self.type_parameter_allows_void(name, &params, &ret),
                )?;
                sub.insert(name.clone(), resolved);
            }
        }

        if let Some(want) = expected.filter(|want| pins_type_parameters(want)) {
            self.bind_from_expected_result(&mut sub, &ret, want);
        }

        let has_rest = params.last().is_some_and(|p| p.rest);
        let fixed_count = params.iter().take_while(|p| !p.rest).count();
        let max_args = if has_rest { usize::MAX } else { params.len() };
        let min_args = params
            .iter()
            .take_while(|p| !p.rest && p.default.is_none())
            .count();
        let arity_ok = args.len() >= min_args && args.len() <= max_args;
        if !arity_ok {
            let help = self.generic_callee_lift(callee, &callee_ident, &generics, &params, &ret);
            let subject = callee.subject(&callee_ident.name, &generics);
            let msg = if has_rest {
                format!(
                    "{subject} expects {}+ argument(s), got {}",
                    min_args,
                    args.len(),
                )
            } else if min_args == max_args {
                format!(
                    "{subject} expects {} argument(s), got {}",
                    max_args,
                    args.len()
                )
            } else {
                format!(
                    "expected {}-{} argument(s), got {}",
                    min_args,
                    max_args,
                    args.len(),
                )
            };
            self.error_with_help(span, msg, vec![help]);
        }

        let rest_elem_ty: Type = if has_rest {
            match &params
                .last()
                .ok_or_else(|| super::inference_failure("rest signature has no parameters"))?
                .ty
            {
                Type::Array(elem) => (**elem).clone(),
                _ => Type::Error,
            }
        } else {
            Type::Error
        };

        let signature_help = |this: &mut Self| {
            this.generic_callee_lift(callee, &callee_ident, &generics, &params, &ret)
        };
        let errors_before_args = self.error_count();
        let inferred_generics: &[String] = if type_args_written { &[] } else { &generics };
        let mut typed_args = self.infer_generic_arguments(
            &args,
            &params,
            &ret,
            inferred_generics,
            &rest_elem_ty,
            &mut sub,
            signature_help,
        )?;

        if has_rest || typed_args.len() < params.len() {
            self.typed_ast
                .record_authored_arguments(span, typed_args.clone());
        }
        if arity_ok {
            self.fill_omitted_defaults(&params, args.len(), span, &mut typed_args)?;
        }
        // Packed rest array has type T[] — a composite, not a bare TypeVar —
        // so the GenericArgument zip tags it is_generic: false.
        if arity_ok && has_rest {
            self.pack_rest_tail(fixed_count, rest_elem_ty.clone(), span, &mut typed_args)?;
        }

        // `session.get(key)` without a type argument keeps its pre-generic
        // meaning: an unchecked read of `unknown` the caller narrows with a
        // runtime-checked `as`. Only a *written* `<unknown>` is an error, so
        // bind the parameter here rather than letting it go unbound and trip
        // both the inference error and the erasure gate below.
        if checked_get && !type_args_written {
            bind_remaining(&mut sub, &generics, Type::Unknown, &self.type_limits)
                .map_err(type_limit_at(span))?;
        }
        // Same reasoning for `llm.call(model, prompt)`, but the untyped default
        // is the `Completion` envelope rather than `unknown`: an untyped call
        // is not an unchecked read, it is a different fully-typed result. `T`
        // appears only in the return type, so it is never inferable from an
        // argument and would otherwise always trip the unbound-parameter error.
        if checked_llm && !type_args_written {
            bind_remaining(
                &mut sub,
                &generics,
                crate::stdlib::llm::declaration::untyped_result_type(&mangled),
                &self.type_limits,
            )
            .map_err(type_limit_at(span))?;
        }

        sub.bind_whole_union_fallbacks();
        self.check_close_matches(&mut sub, span);
        self.bind_leftover_type_parameters(&mut sub, &generics, &ret, expected, errors_before_args);
        self.bind_uninferred_to_unknown(&mut sub, &generics, &mangled, span)?;
        if let Err(unbound) = sub
            .resolve_all(&generics, &self.type_limits)
            .map_err(type_limit_at(span))?
        {
            self.error_with_help(
                span,
                format!(
                    "cannot infer type parameter(s) {}; consider adding an explicit type argument",
                    unbound
                        .iter()
                        .map(|n| format!("`{n}`"))
                        .collect::<Vec<_>>()
                        .join(", "),
                ),
                vec![callee.explicit_arg_hint(
                    &callee_ident.name,
                    &generics,
                    &sub,
                    &self.type_limits,
                )],
            );
        }

        self.check_inferred_void_arguments(&params, &ret, &sub, span);
        let result_ty = self.instantiate(&sub, &ret, span)?;

        // A typed `llm.call<T>` is rewritten before the argument list is frozen,
        // because the schema emitted from `T` has to replace the trailing
        // `schema` argument the default filled with `null`. The rewrite can also
        // fail (an unschemable or unverifiable `T`), in which case the whole
        // call is already an error and no node is built.
        // The `schema` slot is compiler-filled, never program-written. Rejecting
        // a written one is a soundness requirement, not tidiness: the host keys
        // "this call is typed" off that argument being non-null
        // (`stdlib/llm/mod.rs`), while the structural check rides on the cast
        // emitted here. A hand-written schema sets the first without the second,
        // so raw parsed JSON reaches the guest wearing the `Completion`
        // interface type — reading `ok` off a JSON object yields a fabricated
        // `true`, and reading `text` traps outside the error taxonomy. It also
        // hands guest-controlled bytes to the provider as the schema.
        //
        // This rejects rather than ignoring the argument, because silently
        // discarding what a program wrote is its own trap. Typed calls are
        // unaffected in behavior — `substitute_schema_argument` overwrites the
        // slot — but a written argument there is equally meaningless, so both
        // forms are refused and no working program changes.
        if checked_llm && args.len() > 2 {
            let name = &callee_ident.name;
            self.error_with_help(
                span,
                format!(
                    "`{name}` takes the model and the prompt; the schema argument is filled by \
                     the compiler from the type argument"
                ),
                vec![format!(
                    "drop the third argument — write `{name}<T>(...)` to send a schema for `T`, \
                     or `{name}(...)` for an untyped call"
                )],
            );
            return Ok((TypedExprKind::Null, Type::Error));
        }

        let mut llm_schema = None;
        if checked_llm {
            match self.llm_call_schema(&result_ty, type_args_written, span) {
                Ok(schema) => llm_schema = schema,
                Err(()) => return Ok((TypedExprKind::Null, Type::Error)),
            }
        }
        if let Some(schema) = &llm_schema {
            self.substitute_schema_argument(&params, &mut typed_args, schema, span)?;
        }
        self.record_generic_call_arguments(&typed_args, &params, &ret, type_args_written);

        let generic_args: Vec<crate::GenericArgument> = typed_args
            .into_iter()
            .zip(params.iter())
            .map(|(expr, p)| crate::GenericArgument {
                expr,
                is_generic: matches!(p.ty, Type::TypeVar(_)),
            })
            .collect();
        let return_cast = if return_erases_to_object_slot(&ret) {
            Some(result_ty.clone())
        } else {
            None
        };
        let type_predicate = match predicate {
            Some(p) => Some(Box::new(crate::TypePredicate {
                parameter_index: p.parameter_index,
                asserted_type: self.instantiate(&sub, &p.asserted_type, span)?,
            })),
            None => None,
        };
        // A checked read must not keep `return_cast`: it is a representation
        // cast that tests nothing, and on a stored or missing `null` its
        // `ref.cast` traps uncatchably before any structural check could run.
        // The call node is left producing `unknown` and the checked `Cast`
        // wrapped around it does the verifying. Intercepting here rather than
        // at a callsite is what covers every import form, since all four
        // callers thread the package-export mangled name through unchanged.
        let runtime_args: Vec<_> = generics
            .iter()
            .map(|name| self.instantiate(&sub, &Type::TypeVar(name.clone()), span))
            .collect::<Result<_, _>>()?;
        for arg in &runtime_args {
            self.record_runtime_type_test(arg)?;
        }
        let call = TypedExprKind::GenericCall {
            mangled,
            type_args: runtime_args,
            args: generic_args,
            return_cast: if checked_get || checked_llm {
                None
            } else {
                return_cast
            },
            type_predicate,
        };
        if checked_get {
            return self.checked_session_get(call, &result_ty, type_args_written, span);
        }
        // An untyped `llm.call` emitted no schema and needs no check: it returns
        // the `Completion` envelope the declaration already typed.
        if checked_llm && llm_schema.is_some() {
            return self.checked_llm_cast(call, &result_ty, span);
        }
        Ok((call, result_ty))
    }
}

/// What every argument of one generic call is inferred with.
struct GenericArguments<'a> {
    /// The type parameters the call infers rather than takes as written.
    inferred_generics: &'a [String],
    /// Those that only object and array literals are candidates for
    /// ([`super::inference_sources`]).
    sourced_by_literals: Vec<String>,
    literal_types: LiteralTypeArguments,
}

/// What a call does with the literal type of an argument passed straight for
/// one of its inferred type parameters, as tsc does: a parameter the result
/// is (`id<T>(x: T): T`, or a union naming it) keeps the literal (`id(1)` is
/// `1`), and any other widens a fresh one (`box(c)` with `const c = "a"`
/// is a `{ v: string }`), so the result can hold other values. An annotated
/// literal type is not fresh and stays.
///
/// This applies only to a type parameter every parameter names at its top
/// level: one nested in another (`append<T>(a: T[], x: T)`) has candidates
/// tsc doesn't widen. A literal is kept only when its argument is the type
/// parameter's sole candidate: tsc would infer a union of several, which a
/// conflicting binding can't express, so those widen (`two(c, "x")` binds
/// `string`).
struct LiteralTypeArguments {
    kept: Vec<String>,
    widened: Vec<String>,
}

impl LiteralTypeArguments {
    fn new(args: &[(ExprId, Type)], ret: &Type, inferred_generics: &[String]) -> Self {
        let in_result = top_level_type_params(ret);
        let mut kept = Vec::new();
        let mut widened = Vec::new();
        for name in inferred_generics {
            let mentioning: Vec<&Type> = args
                .iter()
                .map(|(_, param)| param)
                .filter(|param| super::expr::mentions_type_var(param, &|var| var == name))
                .collect();
            let only_top_level = mentioning
                .iter()
                .all(|param| top_level_type_params(param).contains(&name.as_str()));
            if mentioning.is_empty() || !only_top_level {
                continue;
            }
            let sole_candidate = mentioning.len() == 1;
            if sole_candidate && in_result.contains(&name.as_str()) {
                kept.push(name.clone());
            } else {
                widened.push(name.clone());
            }
        }
        Self { kept, widened }
    }

    /// Whether an argument for `param` keeps its literal type.
    fn keeps(&self, param: &Type) -> bool {
        top_level_type_params(param)
            .iter()
            .any(|name| self.kept.iter().any(|kept| kept == name))
    }

    /// Whether an argument for `param` widens its fresh literal types.
    fn widens(&self, param: &Type) -> bool {
        top_level_type_params(param)
            .iter()
            .any(|name| self.widened.iter().any(|widened| widened == name))
    }
}

/// The type parameters `ty` is, alone or as a member of a union.
fn top_level_type_params(ty: &Type) -> Vec<&str> {
    match ty.peel() {
        Type::Union(members) => members.iter().filter_map(type_param_name).collect(),
        other => type_param_name(other).into_iter().collect(),
    }
}

fn type_param_name(ty: &Type) -> Option<&str> {
    match ty.peel() {
        Type::TypeVar(name) => Some(name),
        _ => None,
    }
}

/// How many interfaces one literal argument's hint expands at most per pass
/// of [`expand_hint_interfaces`], apart from the first pass, which may also
/// expand as many again at the top union level. Interfaces that name each
/// other in their fields expand along every order of them, so without a
/// bound the hint grows factorially, and checking a literal against a large
/// structural hint takes time in proportion to its size; a literal's hint
/// needs few levels in practice.
const MAX_HINT_INTERFACE_EXPANSIONS: usize = 64;

/// How many unions deep one literal argument's hint expands at most.
const MAX_HINT_UNION_DEPTH: usize = 8;

/// The state of one [`expand_inferred_interfaces`] walk.
struct InterfaceExpansion {
    /// The interfaces being expanded, outermost first.
    expanding: Vec<crate::MangledName>,
    /// How many more interfaces may expand under `max_union_depth` unions.
    remaining_at_max_depth: usize,
    /// How many more interfaces may expand under fewer unions.
    remaining_shallower: usize,
    /// How many interfaces have expanded.
    expansion_count: usize,
    /// How many unions enclose the type being expanded.
    union_depth: usize,
    /// How many enclosing unions an interface may have and still expand.
    max_union_depth: usize,
    /// Whether an interface stayed as it is for want of budget.
    starved: bool,
    /// Whether an interface stayed as it is for being nested in more unions
    /// than `max_union_depth`.
    cut_at_union_depth: bool,
}

impl InterfaceExpansion {
    /// A walk that expands up to `remaining_at_max_depth` interfaces under
    /// `max_union_depth` unions, after those under fewer.
    fn new(max_union_depth: usize, remaining_at_max_depth: usize) -> Self {
        Self {
            expanding: Vec::new(),
            remaining_at_max_depth,
            remaining_shallower: MAX_HINT_INTERFACE_EXPANSIONS,
            expansion_count: 0,
            union_depth: 0,
            max_union_depth,
            starved: false,
            cut_at_union_depth: false,
        }
    }

    /// The budget an interface at the current union depth expands from.
    fn budget(&mut self) -> &mut usize {
        if self.union_depth < self.max_union_depth {
            &mut self.remaining_shallower
        } else {
            &mut self.remaining_at_max_depth
        }
    }

    /// Whether an interface at the current union depth may expand, noting
    /// when one may not.
    fn can_expand(&mut self) -> bool {
        let available = *self.budget() > 0;
        self.starved |= !available;
        available
    }

    /// Count an interface at the current union depth as expanded.
    fn spend(&mut self) {
        *self.budget() -= 1;
        self.expansion_count += 1;
    }
}

/// `ty` expanded by [`expand_inferred_interfaces`] through as many levels of
/// nested unions as the budget covers. Each pass re-expands what the pass
/// before did and gives the interfaces one union deeper the rest of
/// [`MAX_HINT_INTERFACE_EXPANSIONS`]; the passes stop once one runs out of
/// budget, cuts nothing at its depth, or leaves no budget for the next. So
/// the members of an outer union all expand before any member of a union
/// inside them: a wide union of interfaces that name it again
/// (`Lit<T> | Add<T> | …`) expands its own members rather than the first
/// member's descendants.
fn expand_hint_interfaces(
    ty: &Type,
    inferred_generics: &[String],
    types: super::assignable::TypeResolver,
) -> Type {
    let mut expansion = InterfaceExpansion::new(1, MAX_HINT_INTERFACE_EXPANSIONS);
    let mut deepest = expand_inferred_interfaces(ty, inferred_generics, types, &mut expansion);
    for max_union_depth in 2..=MAX_HINT_UNION_DEPTH {
        let remaining = MAX_HINT_INTERFACE_EXPANSIONS.saturating_sub(expansion.expansion_count);
        if expansion.starved || !expansion.cut_at_union_depth || remaining == 0 {
            break;
        }
        expansion = InterfaceExpansion::new(max_union_depth, remaining);
        deepest = expand_inferred_interfaces(ty, inferred_generics, types, &mut expansion);
    }
    deepest
}

/// `ty` with each data-only interface that names one of `inferred_generics`
/// replaced by its fields, through object fields, array and tuple elements and
/// union members: the positions a literal's own fields and elements take
/// their hints from. An interface stays as it is when met again inside its
/// own fields (so a recursive one expands once), when more unions than the
/// walk allows enclose it, or once the walk has used its budget.
fn expand_inferred_interfaces(
    ty: &Type,
    inferred_generics: &[String],
    types: super::assignable::TypeResolver,
    expansion: &mut InterfaceExpansion,
) -> Type {
    let expand = |inner: &Type, expansion: &mut InterfaceExpansion| {
        expand_inferred_interfaces(inner, inferred_generics, types, expansion)
    };
    match ty.peel() {
        interface @ Type::InterfaceRef { mangled, .. }
            if !expansion.expanding.contains(mangled)
                && super::expr::mentions_type_var(interface, &|var| {
                    inferred_generics.iter().any(|name| name == var)
                }) =>
        {
            if expansion.union_depth > expansion.max_union_depth {
                expansion.cut_at_union_depth = true;
                return ty.clone();
            }
            if !expansion.can_expand() {
                return ty.clone();
            }
            let Some(shape) = super::assignable::expand_interface_data_shape(interface, types)
            else {
                return ty.clone();
            };
            expansion.spend();
            expansion.expanding.push(mangled.clone());
            let expanded = expand(&shape, expansion);
            expansion.expanding.pop();
            expanded
        }
        Type::Object { fields, index } => Type::Object {
            fields: fields
                .iter()
                .map(|(name, field)| {
                    let ty = expand(&field.ty, expansion);
                    (
                        name.clone(),
                        crate::types::ObjectField {
                            ty,
                            ..field.clone()
                        },
                    )
                })
                .collect(),
            index: index.clone(),
        },
        Type::Array(element) => Type::Array(Box::new(expand(element, expansion))),
        Type::Tuple(elements) => Type::Tuple(
            elements
                .iter()
                .map(|element| expand(element, expansion))
                .collect(),
        ),
        Type::Union(members) => {
            expansion.union_depth += 1;
            let expanded = members
                .iter()
                .map(|member| expand(member, expansion))
                .collect();
            expansion.union_depth -= 1;
            Type::union(expanded)
        }
        _ => ty.clone(),
    }
}

/// The function type a parameter holds: itself through aliases, or the one
/// function member of a union such as `((x: T) => U) | null`.
fn function_part(ty: &Type) -> Option<&Type> {
    match ty.peel() {
        function @ Type::Function { .. } => Some(function),
        Type::Union(members) => {
            let mut functions = members
                .iter()
                .map(Type::peel)
                .filter(|member| matches!(member, Type::Function { .. }));
            let function = functions.next()?;
            functions.next().is_none().then_some(function)
        }
        _ => None,
    }
}

/// Bind the type parameters a deferred callback's parameters take that no
/// other argument inferred to their whole-union fallback, or to `unknown`
/// when there is none, as tsc fixes them before typing the callback:
/// `Array.from({ length: 3 }, (_, i) => i)` types `_` as `unknown`.
fn fix_callback_parameters(sub: &mut TypeParamSubstitution, param_ty: &Type, inferred: &[String]) {
    let Some(Type::Function { params, .. }) = function_part(param_ty) else {
        return;
    };
    for name in inferred {
        let taken = params
            .iter()
            .any(|param| super::expr::mentions_type_var(param, &|var| var == name));
        if taken && sub.get(name).is_none() {
            let fixed = sub.whole_union_fallback(name).cloned();
            sub.insert(name.clone(), fixed.unwrap_or(Type::Unknown));
        }
    }
}

/// An object literal argument whose fields bind a generic call's type
/// parameters one at a time, for the fields after them.
pub(crate) struct ObjectArgumentInference {
    literal: ExprId,
    /// The parameter's fields, in terms of the type parameters.
    fields: std::collections::BTreeMap<String, crate::ObjectField>,
    sub: TypeParamSubstitution,
    /// Type parameters a callback field bound to just their whole-union
    /// fallback, left unbound so a later field may bind them; see
    /// [`TypeParamSubstitution::unbind_fallback_echoes`].
    fallback_echoes: Vec<String>,
}

/// A coarse category of value, used to skip the union members a call's result
/// can never be (a function result and `null`). `Uint8Array` is a primitive
/// here: no generic result type can be one.
#[derive(Clone, Copy, PartialEq)]
enum ShapeKind {
    Function,
    /// An object, interface, class instance, array or tuple.
    Structured,
    Primitive,
    /// A type parameter, `unknown`, an error: anything.
    Any,
}

impl ShapeKind {
    fn of(ty: &Type) -> Self {
        match ty.peel() {
            Type::Function { .. } => Self::Function,
            // An array is assignable to an interface or object type of the
            // fields it has (`{ length: number }`), so it is one kind with them.
            Type::Object { .. }
            | Type::InterfaceRef { .. }
            | Type::ClassRef { .. }
            | Type::Array(_)
            | Type::Tuple(_) => Self::Structured,
            Type::Number
            | Type::NumberLiteral(_)
            | Type::BigInt
            | Type::String
            | Type::StringLiteral(_)
            | Type::Uint8Array
            | Type::Boolean
            | Type::BooleanLiteral(_)
            | Type::NumberEnum { .. }
            | Type::StringEnum { .. }
            | Type::Null
            | Type::Void
            | Type::Never => Self::Primitive,
            _ => Self::Any,
        }
    }

    fn could_be(self, other: Self) -> bool {
        self == other || self == Self::Any || other == Self::Any
    }
}

/// Whether a call's expected result type is worth binding its type parameters
/// from before the arguments are inferred. `unknown` is not: every type is
/// assignable to it, so binding a parameter to it first would hide what the
/// arguments say (`console.log(xs.reduce((a, b) => a + b, 0))` would type `a`
/// as `unknown`). It only fills the parameters the arguments leave unbound.
fn pins_type_parameters(want: &Type) -> bool {
    !matches!(want.peel(), Type::Error | Type::Unknown)
}

/// Bind every type parameter inference left unsolved to `fallback`, so the call
/// resolves at a default rather than erroring as uninferable. Already-bound
/// parameters keep what inference gave them.
fn bind_remaining(
    sub: &mut crate::typechecker::type_param_substitution::TypeParamSubstitution,
    generics: &[String],
    fallback: Type,
    limits: &TypeLimits,
) -> Result<(), TypeTooLarge> {
    if let Err(unbound) = sub.resolve_all(generics, limits)? {
        for name in unbound {
            sub.insert(name, fallback.clone());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitute_typevars_replaces_top_level_typevar() {
        let mut bindings: BTreeMap<String, Type> = BTreeMap::new();
        bindings.insert("T".into(), Type::Number);
        let out = substitute_typevars(
            &Type::TypeVar("T".into()),
            &bindings,
            &crate::type_size::TypeLimits::default(),
        )
        .unwrap();
        assert_eq!(out, Type::Number);
    }

    #[test]
    fn substitute_typevars_recurses_into_nested_constructors() {
        let mut bindings: BTreeMap<String, Type> = BTreeMap::new();
        bindings.insert("T".into(), Type::String);
        let arr = Type::Array(Box::new(Type::TypeVar("T".into())));
        let out =
            substitute_typevars(&arr, &bindings, &crate::type_size::TypeLimits::default()).unwrap();
        assert_eq!(out, Type::Array(Box::new(Type::String)));
        let func = Type::Function {
            params: vec![Type::TypeVar("T".into())],
            ret: Box::new(Type::TypeVar("T".into())),
            predicate: None,
            has_rest: false,
        };
        let out = substitute_typevars(&func, &bindings, &crate::type_size::TypeLimits::default())
            .unwrap();
        assert_eq!(
            out,
            Type::Function {
                params: vec![Type::String],
                ret: Box::new(Type::String),
                predicate: None,
                has_rest: false,
            }
        );
    }

    #[test]
    fn substitute_typevars_leaves_unbound_names_alone() {
        let mut bindings: BTreeMap<String, Type> = BTreeMap::new();
        bindings.insert("T".into(), Type::Number);
        let out = substitute_typevars(
            &Type::TypeVar("U".into()),
            &bindings,
            &crate::type_size::TypeLimits::default(),
        )
        .unwrap();
        assert_eq!(out, Type::TypeVar("U".into()));
    }

    #[test]
    fn substitute_typevars_charges_each_copy_of_a_binding() {
        crate::type_size::tests::on_compiler_stack(
            substitute_typevars_charges_each_copy_of_a_binding_inner,
        );
    }

    fn substitute_typevars_charges_each_copy_of_a_binding_inner() {
        use crate::compiler_limits::{MAX_TYPE_DEPTH, MAX_TYPE_NODES};
        use crate::type_size::{TypeLimits, TypeTooLarge};
        let pair = Type::Tuple(vec![Type::TypeVar("T".into()), Type::TypeVar("T".into())]);
        let limits = TypeLimits::default();
        // The tuple itself plus two copies of the binding, each a tuple of
        // numbers: MAX_TYPE_NODES - 1 nodes, then MAX_TYPE_NODES + 1.
        let half = Type::Tuple(vec![Type::Number; (MAX_TYPE_NODES / 2 - 2) as usize]);
        let bindings = BTreeMap::from([("T".to_string(), half)]);
        assert!(substitute_typevars(&pair, &bindings, &limits).is_ok());
        let over = Type::Tuple(vec![Type::Number; (MAX_TYPE_NODES / 2 - 1) as usize]);
        let bindings = BTreeMap::from([("T".to_string(), over)]);
        assert_eq!(
            substitute_typevars(&pair, &bindings, &limits),
            Err(TypeTooLarge::Nodes)
        );
        let deep = (1..MAX_TYPE_DEPTH).fold(Type::Number, |inner, _| Type::Array(Box::new(inner)));
        let bindings = BTreeMap::from([("T".to_string(), deep)]);
        assert_eq!(
            substitute_typevars(&pair, &bindings, &limits),
            Err(TypeTooLarge::Depth)
        );
    }

    use super::super::test_support::{run, run_clean};
    use crate::{ExprId, PackageDeclaration, Param, StmtId, TypedAst, TypedStmtKind, ValueKind};

    fn last_call_resolved_ty(ta: &TypedAst) -> Type {
        for &id in ta.top_level_statements.iter().rev() {
            if let TypedStmtKind::AssignGlobal { value, .. } = &ta.try_stmt(id).unwrap().kind {
                return ta.try_expr(*value).unwrap().ty.clone();
            }
        }
        panic!("no top-level let/const init found");
    }

    #[test]
    fn hof_map_typechecks_closure_arg() {
        let (ta, diags) = run(r#"
            function map<T, U>(arr: T[], fn: (x: T) => U): U[] {
                return [];
            }
            let result = map([1, 2, 3], (x: number): string => x.toString());
            "#);
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        let ty = last_call_resolved_ty(&ta);
        assert_eq!(
            ty,
            Type::Array(Box::new(Type::String)),
            "expected string[], got {ty:?}",
        );
    }

    #[test]
    fn hof_filter_typechecks_predicate() {
        let (ta, diags) = run(r#"
            function filter<T>(arr: T[], pred: (x: T) => boolean): T[] {
                return [];
            }
            let result = filter(["a", "b"], (x: string): boolean => x === "a");
            "#);
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        let ty = last_call_resolved_ty(&ta);
        assert_eq!(ty, Type::Array(Box::new(Type::String)));
    }

    #[test]
    fn hof_reduce_typechecks_with_accumulator() {
        let (ta, diags) = run(r#"
            function reduce<T, U>(arr: T[], fn: (acc: U, x: T) => U, init: U): U {
                return init;
            }
            let result = reduce([1, 2, 3], (acc: number, x: number): number => acc + x, 0);
            "#);
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        let ty = last_call_resolved_ty(&ta);
        assert_eq!(ty, Type::Number);
    }

    #[test]
    fn hof_bidirectional_return_seeds_t_from_lhs_annotation() {
        let (ta, diags) = run(r#"
            function none<T>(): T[] {
                return [];
            }
            let result: number[] = none();
            "#);
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        let ty = last_call_resolved_ty(&ta);
        assert_eq!(ty, Type::Array(Box::new(Type::Number)));
    }

    fn collect_gps(ty: &Type, out: &mut Vec<u32>) {
        match ty {
            Type::GenericParam { id, .. } => out.push(*id),
            Type::Array(elem) => collect_gps(elem, out),
            Type::Function { params, ret, .. } => {
                for p in params {
                    collect_gps(p, out);
                }
                collect_gps(ret, out);
            }
            Type::Object { fields, .. } => {
                for v in fields.values() {
                    collect_gps(&v.ty, out);
                }
            }
            _ => {}
        }
    }

    fn collect_body_expr_types(ta: &TypedAst, stmt_id: StmtId, out: &mut Vec<Type>) {
        let stmt = ta.try_stmt(stmt_id).unwrap();
        match &stmt.kind {
            TypedStmtKind::Block(children) => {
                for &c in children {
                    collect_body_expr_types(ta, c, out);
                }
            }
            TypedStmtKind::Let { value, .. }
            | TypedStmtKind::Const { value, .. }
            | TypedStmtKind::AssignLocal { value, .. }
            | TypedStmtKind::AssignGlobal { value, .. } => {
                collect_expr_types(ta, *value, out);
            }
            TypedStmtKind::AssignField {
                receiver, value, ..
            } => {
                collect_expr_types(ta, *receiver, out);
                collect_expr_types(ta, *value, out);
            }
            TypedStmtKind::AssignIndex {
                receiver,
                index,
                value,
                ..
            } => {
                collect_expr_types(ta, *receiver, out);
                collect_expr_types(ta, *index, out);
                collect_expr_types(ta, *value, out);
            }
            TypedStmtKind::Return(Some(e)) | TypedStmtKind::Expr(e) => {
                collect_expr_types(ta, *e, out);
            }
            TypedStmtKind::Return(None) => {}
            TypedStmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                collect_expr_types(ta, *condition, out);
                collect_body_expr_types(ta, *then_block, out);
                if let Some(eb) = else_block {
                    collect_body_expr_types(ta, *eb, out);
                }
            }
            TypedStmtKind::While { condition, body } => {
                collect_expr_types(ta, *condition, out);
                collect_body_expr_types(ta, *body, out);
            }
            TypedStmtKind::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    collect_body_expr_types(ta, *i, out);
                }
                if let Some(c) = condition {
                    collect_expr_types(ta, *c, out);
                }
                if let Some(u) = update {
                    collect_body_expr_types(ta, *u, out);
                }
                collect_body_expr_types(ta, *body, out);
            }
            TypedStmtKind::ForOf { iter, body, .. } => {
                collect_expr_types(ta, *iter, out);
                collect_body_expr_types(ta, *body, out);
            }
            TypedStmtKind::DoWhile { body, condition } => {
                collect_body_expr_types(ta, *body, out);
                collect_expr_types(ta, *condition, out);
            }
            TypedStmtKind::Switch {
                discriminant,
                cases,
                default,
                ..
            } => {
                collect_expr_types(ta, *discriminant, out);
                for case in cases {
                    collect_body_expr_types(ta, case.body, out);
                }
                if let Some(d) = default {
                    collect_body_expr_types(ta, *d, out);
                }
            }
            TypedStmtKind::Break | TypedStmtKind::Continue | TypedStmtKind::ReboxLocal { .. } => {}
            TypedStmtKind::NarrowRegion { source, body, .. } => {
                collect_expr_types(ta, *source, out);
                collect_body_expr_types(ta, *body, out);
            }
            TypedStmtKind::Throw { value } => collect_expr_types(ta, *value, out),
            TypedStmtKind::Try {
                body,
                catches,
                finally,
            } => {
                collect_body_expr_types(ta, *body, out);
                for c in catches {
                    collect_body_expr_types(ta, c.body, out);
                }
                if let Some(f) = finally {
                    collect_body_expr_types(ta, *f, out);
                }
            }
        }
    }

    fn collect_expr_types(ta: &TypedAst, expr_id: ExprId, out: &mut Vec<Type>) {
        let expr = ta.try_expr(expr_id).unwrap();
        out.push(expr.ty.clone());
        match &expr.kind {
            crate::TypedExprKind::Binary { lhs, rhs, .. } => {
                collect_expr_types(ta, *lhs, out);
                collect_expr_types(ta, *rhs, out);
            }
            crate::TypedExprKind::Unary { operand, .. } => {
                collect_expr_types(ta, *operand, out);
            }
            crate::TypedExprKind::Call { args, .. } | TypedExprKind::McpCall { args, .. } => {
                for a in args {
                    collect_expr_types(ta, *a, out);
                }
            }
            crate::TypedExprKind::CallClosure { callee, args } => {
                collect_expr_types(ta, *callee, out);
                for a in args {
                    collect_expr_types(ta, *a, out);
                }
            }
            crate::TypedExprKind::IntrinsicCall { args, .. } => {
                for a in args {
                    collect_expr_types(ta, *a, out);
                }
            }
            crate::TypedExprKind::FieldAccess { receiver, .. } => {
                collect_expr_types(ta, *receiver, out);
            }
            crate::TypedExprKind::IndexAccess { receiver, index } => {
                collect_expr_types(ta, *receiver, out);
                collect_expr_types(ta, *index, out);
            }
            crate::TypedExprKind::ObjectLiteral { members, .. } => {
                for member in members {
                    for expression in member.expressions() {
                        collect_expr_types(ta, expression, out);
                    }
                }
            }
            crate::TypedExprKind::ArrayLiteral { elements, .. } => {
                for e in elements {
                    collect_expr_types(ta, e.expr_id(), out);
                }
            }
            _ => {}
        }
    }

    fn nth_function(ta: &TypedAst, n: usize) -> &crate::TypedFunction {
        &ta.functions[n]
    }

    #[test]
    fn generic_params_erased_to_typevar_after_inference() {
        let ta = run_clean(
            r#"
            function f<T>(x: T): T {
                let y: T = x;
                return y;
            }
            "#,
        );
        let defs = PackageDeclaration::from_typed_ast(&ta);
        let f = defs.values.get("f").expect("f registered");
        match &f.kind {
            ValueKind::Function {
                generics,
                params,
                ret,
                ..
            } => {
                assert_eq!(generics, &vec!["T".to_string()]);
                assert_eq!(
                    params,
                    &vec![Param::new("x", Type::TypeVar("T".to_string()))]
                );
                assert_eq!(*ret, Type::TypeVar("T".to_string()));
            }
            other => panic!("expected ValueKind::Function, got {other:?}"),
        }
        let f = nth_function(&ta, 0);
        let typed_params = &f.params;
        let typed_ret = f.return_type.clone();
        let body = f.body;
        for p in typed_params {
            assert!(
                matches!(p.ty, Type::TypeVar(_)),
                "param `{}`: expected TypeVar, got {:?}",
                p.name.name,
                p.ty,
            );
        }
        assert!(
            matches!(typed_ret, Type::TypeVar(_)),
            "return: expected TypeVar, got {typed_ret:?}",
        );
        let mut body_tys = Vec::new();
        collect_body_expr_types(&ta, body, &mut body_tys);
        let saw_typevar = body_tys.iter().any(|t| matches!(t, Type::TypeVar(_)));
        assert!(
            saw_typevar,
            "body should contain TypeVar after erasure, got {body_tys:?}"
        );
        let mut gp_ids = Vec::new();
        for t in &body_tys {
            collect_gps(t, &mut gp_ids);
        }
        assert!(
            gp_ids.is_empty(),
            "body should have no GenericParam after erasure, got ids {gp_ids:?}"
        );
    }

    #[test]
    fn cross_scope_generic_bodies_have_no_gp_after_erasure() {
        let ta = run_clean(
            r#"
            function inner<U>(y: U): U { return y; }
            function outer<T>(x: T): T { return inner(x); }
            "#,
        );
        let defs = PackageDeclaration::from_typed_ast(&ta);
        for fn_name in ["inner", "outer"] {
            match &defs.values.get(fn_name).unwrap().kind {
                ValueKind::Function { params, ret, .. } => {
                    for p in params {
                        assert!(
                            matches!(p.ty, Type::TypeVar(_)),
                            "{fn_name}'s param should be TypeVar, got {:?}",
                            p.ty,
                        );
                    }
                    assert!(
                        matches!(ret, Type::TypeVar(_)),
                        "{fn_name}'s ret should be TypeVar, got {ret:?}"
                    );
                }
                _ => panic!(),
            }
        }
        for n in 0..2 {
            let body = nth_function(&ta, n).body;
            let mut body_tys = Vec::new();
            collect_body_expr_types(&ta, body, &mut body_tys);
            let mut gp_ids = Vec::new();
            for t in &body_tys {
                collect_gps(t, &mut gp_ids);
            }
            assert!(
                gp_ids.is_empty(),
                "function #{n}'s body has GenericParam ids {gp_ids:?} after erasure",
            );
        }
    }

    #[test]
    fn conflict_diagnostic_uses_real_names_not_garbled() {
        let (_, diags) = run(r#"
            function pair<T>(a: T, b: T): T { return a; }
            function main(): void {
                pair(1, "x");
            }
            "#);
        assert!(!diags.is_empty(), "expected at least one diagnostic");
        let saw_clean = diags.iter().any(|d| {
            (d.message.contains("number") && d.message.contains("string"))
                && !d.message.contains("to `T`")
        });
        assert!(
            saw_clean,
            "expected clean conflict diagnostic, got: {diags:#?}",
        );
    }

    #[test]
    fn cross_scope_gp_assignment_rejects_with_clean_message() {
        let (_, diags) = run(r#"
            function identity<T>(x: T): T { return x; }
            function bar<T, U>(y: U): void {
                const x: T = identity(y);
            }
            "#);
        assert!(
            !diags.is_empty(),
            "expected rejection of cross-GP assignment"
        );
        let saw_clean = diags.iter().any(|d| {
            d.message.contains('T')
                && d.message.contains('U')
                && !d.message.contains("`T`, cannot bind to `T`")
                && !d.message.contains("`T` already bound to `T`")
        });
        assert!(
            saw_clean,
            "expected clean cross-GP diagnostic mentioning both T and U, got: {diags:#?}",
        );
    }

    #[test]
    fn resolve_all_treats_self_binding_as_unbound() {
        use crate::typechecker::type_param_substitution::TypeParamSubstitution;
        let mut sub = TypeParamSubstitution::new();
        sub.insert("T".to_string(), Type::TypeVar("T".to_string()));
        let result = sub
            .resolve_all(&["T".to_string()], &crate::type_size::TypeLimits::default())
            .unwrap();
        assert!(
            result.is_err(),
            "self-binding T → TypeVar(T) should be reported as unbound; got {result:?}",
        );
    }

    #[test]
    fn array_map_infers_method_level_generic_from_closure() {
        let (ta, diags) = run(r#"
            function main(): void {
                const xs = [1, 2, 3].map((x: number): number => x);
            }
            "#);
        assert!(diags.is_empty(), "expected clean typecheck, got: {diags:?}");
        let body = ta.functions[0].body;
        let TypedStmtKind::Block(stmts) = &ta.try_stmt(body).unwrap().kind else {
            panic!("expected block body");
        };
        let TypedStmtKind::Const { ty, .. } = &ta.try_stmt(stmts[0]).unwrap().kind else {
            panic!("expected const");
        };
        assert_eq!(*ty, Type::Array(Box::new(Type::Number)));
    }

    #[test]
    fn array_map_with_explicit_method_type_arg() {
        let (ta, diags) = run(r#"
            function main(): void {
                const xs = [1, 2, 3].map<number>((x: number): number => x);
            }
            "#);
        assert!(diags.is_empty(), "expected clean typecheck, got: {diags:?}");
        let body = ta.functions[0].body;
        let TypedStmtKind::Block(stmts) = &ta.try_stmt(body).unwrap().kind else {
            panic!("expected block body");
        };
        let TypedStmtKind::Const { ty, .. } = &ta.try_stmt(stmts[0]).unwrap().kind else {
            panic!("expected const");
        };
        assert_eq!(*ty, Type::Array(Box::new(Type::Number)));
    }

    #[test]
    fn array_map_explicit_type_arg_arity_mismatch_diagnoses() {
        let (_, diags) = run(r#"
            function main(): void {
                const xs = [1].map<number, string>((x: number): number => x);
            }
            "#);
        assert!(
            diags.iter().any(|d| d
                .message
                .contains("method `map` expects 1 type argument(s), got 2")),
            "expected type-arg arity diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn array_map_with_user_typed_closure_returns_inferred_array() {
        let (ta, diags) = run(r#"
            function main(): void {
                const xs = [{x: 1}, {x: 2}].map(
                    (o: { x: number }): number => o.x
                );
            }
            "#);
        assert!(diags.is_empty(), "expected clean typecheck, got: {diags:?}");
        let body = ta.functions[0].body;
        let TypedStmtKind::Block(stmts) = &ta.try_stmt(body).unwrap().kind else {
            panic!("expected block body");
        };
        let TypedStmtKind::Const { ty, .. } = &ta.try_stmt(stmts[0]).unwrap().kind else {
            panic!("expected const");
        };
        assert_eq!(*ty, Type::Array(Box::new(Type::Number)));
    }

    #[test]
    fn user_interface_method_generic_typechecks() {
        let (_, diags) = run(r#"
            interface Box<T> {
                map<U>(fn: (x: T) => U): U;
            }
            function main(): void {}
            "#);
        assert!(
            diags.is_empty(),
            "expected clean typecheck of user method-generic interface, got: {diags:?}",
        );
    }

    #[test]
    fn method_generic_shadowing_interface_generic_diagnoses() {
        let (_, diags) = run(r#"
            interface Foo<T> {
                f<T>(): T;
            }
            function main(): void {}
            "#);
        assert!(
            diags.iter().any(|d| d
                .message
                .contains("method generic `T` shadows interface generic `T`")),
            "expected shadowing diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn array_reduce_infers_u_from_init_arg() {
        let (ta, diags) = run(r#"
            function main(): void {
                let nums: number[] = [1, 2, 3];
                let sum = nums.reduce((acc, x) => acc + x, 0);
            }
            "#);
        assert!(
            diags.is_empty(),
            "expected reduce<U> to infer from init arg, got: {diags:?}",
        );
        let body = ta.functions[0].body;
        let TypedStmtKind::Block(stmts) = &ta.try_stmt(body).unwrap().kind else {
            panic!("expected block body");
        };
        let TypedStmtKind::Let { ty, .. } = &ta.try_stmt(stmts[1]).unwrap().kind else {
            panic!("expected let sum");
        };
        assert_eq!(*ty, Type::Number, "sum should infer as number");
    }

    #[test]
    fn array_reduce_infers_u_from_string_init() {
        let (_, diags) = run(r#"
            function main(): void {
                let words = ["a", "b"].reduce((acc, s) => acc + s, "");
            }
            "#);
        assert!(
            diags.is_empty(),
            "expected reduce<U> to infer U := string, got: {diags:?}",
        );
    }

    #[test]
    fn under_constrained_method_generic_is_unknown() {
        let (_, diags) = run(r#"
            interface Box<T> {
                empty<U>(): U[];
            }
            function go(b: Box<number>): void {
                const xs: unknown[] = b.empty();
            }
            "#);
        assert!(
            diags.is_empty(),
            "expected `U` to infer as `unknown`, got: {diags:?}"
        );
    }

    #[test]
    fn rest_param_accepts_zero_trailing_args() {
        let (_, diags) = run(r#"
            function sum(...n: number[]): number { return 0; }
            function main(): void { let x = sum(); }
            "#);
        assert!(diags.is_empty(), "expected clean typecheck, got: {diags:?}");
    }

    #[test]
    fn rest_param_accepts_many_trailing_args() {
        let (_, diags) = run(r#"
            function sum(...n: number[]): number { return 0; }
            function main(): void { let x = sum(1, 2, 3); }
            "#);
        assert!(diags.is_empty(), "expected clean typecheck, got: {diags:?}");
    }

    #[test]
    fn rest_param_rejects_mismatched_trailing_arg() {
        let (_, diags) = run(r#"
            function sum(...n: number[]): number { return 0; }
            function main(): void { let x = sum(1, "x"); }
            "#);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("number") && d.message.contains("string")),
            "expected number-vs-string mismatch, got: {diags:?}",
        );
    }

    #[test]
    fn generic_rest_param_infers_element_type() {
        let (_, diags) = run(r#"
            function pack<T>(...xs: T[]): T[] { return xs; }
            function main(): void { let n = pack(1, 2, 3); let s = pack("a", "b"); }
            "#);
        assert!(diags.is_empty(), "expected clean typecheck, got: {diags:?}");
    }

    #[test]
    fn generic_rest_param_rejects_mixed_element_types() {
        let (_, diags) = run(r#"
            function pack<T>(...xs: T[]): T[] { return xs; }
            function main(): void { let x = pack(1, "a"); }
            "#);
        assert!(
            !diags.is_empty(),
            "expected conflict diagnostic for mixed types"
        );
    }

    #[test]
    fn fixed_plus_rest_accepts_mixed_args() {
        let (_, diags) = run(r#"
            function tag(label: string, ...vals: number[]): string { return label; }
            function main(): void {
                let a = tag("x");
                let b = tag("x", 1, 2);
            }
            "#);
        assert!(diags.is_empty(), "expected clean typecheck, got: {diags:?}");
    }

    #[test]
    fn fixed_plus_rest_requires_prefix_arg() {
        let (_, diags) = run(r#"
            function tag(label: string, ...vals: number[]): string { return label; }
            function main(): void { let x = tag(); }
            "#);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("expected 1+ argument(s), got 0")),
            "expected arity diagnostic with '+' form, got: {diags:?}",
        );
    }
}
