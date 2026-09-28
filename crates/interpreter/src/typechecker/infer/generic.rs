//! Generic inference — body walks for generic and non-generic function decls,
//! call-site type-param unification, and the GP→TypeVar erasure pass.
//!
//! Invariant: `GenericParam`s are allocated in `infer_functions` and live only
//! inside `scopes`/`current_return` during the body walk. Downstream passes
//! (PackageDeclaration, codegen) see only `TypeVar`s.

use std::collections::BTreeMap;

use crate::{
    ExprId, ExprKind, Ident, MethodSig, Span, StmtId, StmtKind, Type, TypeAnnotation,
    TypedExprKind, TypedParam, TypedStmt, TypedStmtKind, ValueKind,
};

use super::Inferer;
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
    ) -> String {
        let args = generics
            .iter()
            .map(|g| {
                let resolved = sub.apply(&Type::TypeVar(g.clone()));
                // Leave an uninferable parameter as its own name for the
                // caller to replace; show the ones inference did settle.
                if matches!(&resolved, Type::TypeVar(n) if n == &g[..]) {
                    g.clone()
                } else {
                    resolved.to_string()
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

pub(super) fn substitute_typevars(ty: &Type, bindings: &BTreeMap<String, Type>) -> Type {
    match ty {
        Type::Refined { original, ty } => Type::Refined {
            original: Box::new(substitute_typevars(original, bindings)),
            ty: Box::new(substitute_typevars(ty, bindings)),
        },
        Type::TypeVar(name) => bindings.get(name).cloned().unwrap_or_else(|| ty.clone()),
        Type::Array(elem) => Type::Array(Box::new(substitute_typevars(elem, bindings))),
        Type::Readonly(inner) => Type::Readonly(Box::new(substitute_typevars(inner, bindings))),
        Type::Tuple(elements) => Type::Tuple(
            elements
                .iter()
                .map(|e| substitute_typevars(e, bindings))
                .collect(),
        ),
        Type::Function {
            params,
            ret,
            predicate,
            has_rest,
        } => Type::Function {
            params: params
                .iter()
                .map(|p| substitute_typevars(p, bindings))
                .collect(),
            ret: Box::new(substitute_typevars(ret, bindings)),
            predicate: predicate.as_ref().map(|p| {
                Box::new(crate::TypePredicate {
                    parameter_index: p.parameter_index,
                    asserted_type: substitute_typevars(&p.asserted_type, bindings),
                })
            }),
            has_rest: *has_rest,
        },
        Type::Object { fields, index } => Type::Object {
            index: index
                .as_ref()
                .map(|i| i.map_value(|v| substitute_typevars(v, bindings))),
            fields: fields
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        crate::ObjectField {
                            ty: substitute_typevars(&v.ty, bindings),
                            optional: v.optional,
                            readonly: v.readonly,
                        },
                    )
                })
                .collect(),
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
            args: args
                .iter()
                .map(|a| substitute_typevars(a, bindings))
                .collect(),
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
            args: args
                .iter()
                .map(|a| substitute_typevars(a, bindings))
                .collect(),
        },
        Type::Union(members) => Type::union(
            members
                .iter()
                .map(|m| substitute_typevars(m, bindings))
                .collect(),
        ),
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
            args: args
                .iter()
                .map(|a| substitute_typevars(a, bindings))
                .collect(),
            ty: Box::new(substitute_typevars(inner, bindings)),
        },
        // A recursion back-edge carries no inline body — only its args
        // need substitution, exactly like `InterfaceRef`.
        Type::AliasRef {
            mangled,
            package,
            name,
            args,
        } => Type::AliasRef {
            mangled: mangled.clone(),
            package: package.clone(),
            name: name.clone(),
            args: args
                .iter()
                .map(|a| substitute_typevars(a, bindings))
                .collect(),
        },
        Type::Number
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
        | Type::StringEnum { .. }
        | Type::GenericParam { .. } => ty.clone(),
    }
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
    ) -> Type {
        let resolved = self.resolve_type(annot);
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
            return Type::Error;
        }
        resolved
    }

    fn type_parameter_allows_void(&self, name: &str, params: &[crate::Param], ret: &Type) -> bool {
        let sub = crate::typechecker::type_param_substitution::TypeParamSubstitution::from_pairs(
            &[name.to_string()],
            &[Type::Void],
        );
        params.iter().all(|p| {
            super::void_type_arguments::invalid_position(&sub.apply(&p.ty), false, self.resolver())
                .is_none()
        }) && super::void_type_arguments::invalid_position(&sub.apply(ret), true, self.resolver())
            .is_none()
    }

    fn check_inferred_void_arguments(
        &mut self,
        params: &[crate::Param],
        ret: &Type,
        sub: &crate::typechecker::type_param_substitution::TypeParamSubstitution,
        span: Span,
    ) {
        let invalid = params
            .iter()
            .find_map(|p| {
                super::void_type_arguments::invalid_position(
                    &sub.apply(&p.ty),
                    false,
                    self.resolver(),
                )
            })
            .or_else(|| {
                super::void_type_arguments::invalid_position(&sub.apply(ret), true, self.resolver())
            });
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
    pub(super) fn infer_functions(&mut self) {
        let top_level: Vec<_> = self.ast.top_level.clone();
        for stmt_id in top_level {
            let stmt = self.ast.stmt(stmt_id).clone();
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
            let body_instantiation = self.push_body_generics(generic_names.clone());
            let body_param_types: Vec<Type> = sig_param_types
                .iter()
                .map(|t| substitute_typevars(t, &body_instantiation))
                .collect();
            let body_ret_type = substitute_typevars(&sig_ret_type, &body_instantiation);
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
                self.scopes
                    .insert(p.name.name.clone(), body_ty.clone(), false, p.name.span);
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
                .infer_stmt(body)
                .expect("function body is a Block, never a type-only decl");
            self.current_return = prev_return;
            self.current_type_predicate = prev_predicate;
            self.reachable = prev_reachable;
            self.scopes.pop();
            self.pop_body_generics();
            for i in exprs_before..self.typed_ast.exprs_len() {
                let id = ExprId(i as u32);
                erase_generic_params_in_expr(self.typed_ast.expr_mut(id));
            }
            for i in stmts_before..self.typed_ast.stmts_len() {
                let id = StmtId(i as u32);
                erase_generic_params_in_stmt(self.typed_ast.stmt_mut(id));
            }
            let ret_type = stored_return;
            let mangled_name = self.mangle_top_symbol(&name.name);
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
            });
        }
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
    ) -> (TypedExprKind, Type) {
        // The no-mapper form preserves the source type; the mapped form has
        // an independent result parameter, like TypeScript's two overloads.
        let mut sig = sig;
        if iface_mangled == crate::mangle::prelude("ArrayConstructor")
            && name.name == "from"
            && (args.len() == 1
                || args
                    .get(1)
                    .is_some_and(|id| matches!(self.ast.expr(*id).kind, crate::ExprKind::Null)))
        {
            sig.generics = vec!["T".into()];
            sig.ret = Type::Array(Box::new(Type::TypeVar("T".into())));
            sig.params[1].ty = Type::Null;
        }

        let receiver_ty = self.typed_ast.expr(typed_receiver).ty.clone();
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
                );
                sub.insert(gname.clone(), resolved);
            }
        }

        if let Some(want) = expected.filter(|want| pins_type_parameters(want)) {
            let _ = sub.unify(&sig.ret, want, self.resolver());
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
            match &sig.params.last().unwrap().ty {
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
        let mut typed_args = self.infer_generic_arguments(
            &args,
            &sig.params,
            &rest_elem_ty,
            &mut sub,
            signature_help,
        );

        if has_rest || typed_args.len() < sig.params.len() {
            self.typed_ast
                .record_authored_arguments(span, typed_args.clone());
        }
        if arity_ok {
            self.fill_omitted_defaults(&sig.params, args.len(), span, &mut typed_args);
        }
        if arity_ok && has_rest {
            self.pack_rest_tail(fixed_count, rest_elem_ty.clone(), span, &mut typed_args);
        }

        let array_from_mapper = iface_mangled == crate::mangle::prelude("ArrayConstructor")
            && name.name == "from"
            && sig.generics.len() == 2;
        let mapper_type = typed_args
            .get(1)
            .map(|id| self.typed_ast.expr(*id).ty.clone());
        if array_from_mapper
            && mapper_type
                .as_ref()
                .is_some_and(|ty| ty.peel() == &Type::Null)
        {
            sub.insert("U".into(), sub.apply(&Type::TypeVar("T".into())));
        }

        self.bind_leftover_type_parameters(
            &mut sub,
            &sig.generics,
            &sig.ret,
            expected,
            errors_before_args,
        );
        if let Err(unbound) = sub.resolve_all(&sig.generics) {
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
        let mut result_ty = sub.apply(&sig.ret);
        if array_from_mapper && mapper_type.as_ref().is_some_and(|ty| {
            matches!(ty.peel(), Type::Union(members) if members.iter().any(|member| member.peel() == &Type::Null))
        }) {
            result_ty = Type::Array(Box::new(Type::union(vec![
                sub.apply(&Type::TypeVar("T".into())),
                sub.apply(&Type::TypeVar("U".into())),
            ])));
        }

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
            let type_predicate = sig.predicate.as_ref().map(|p| {
                Box::new(crate::TypePredicate {
                    parameter_index: p.parameter_index,
                    asserted_type: sub.apply(&p.asserted_type),
                })
            });
            return (
                TypedExprKind::GenericMethodCall {
                    receiver: typed_receiver,
                    iface: iface_mangled,
                    name,
                    args: generic_args,
                    return_cast,
                    type_predicate,
                },
                result_ty,
            );
        }
        // sub carries interface bindings even when the method itself isn't generic.
        let type_predicate = sig.predicate.as_ref().map(|p| {
            Box::new(crate::TypePredicate {
                parameter_index: p.parameter_index,
                asserted_type: sub.apply(&p.asserted_type),
            })
        });
        (
            TypedExprKind::MethodCall {
                receiver: typed_receiver,
                iface: iface_mangled,
                name,
                args: typed_args,
                type_predicate,
            },
            result_ty,
        )
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
        if self.error_count() > errors_before_args {
            bind_remaining(sub, generics, Type::Error);
        }
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
    ) {
        let Some(Type::Function { params, .. }) = function_part(param_ty) else {
            return;
        };
        let Some(declared_params) = self.function_literal_params(literal) else {
            return;
        };
        let diagnostics_before = self.diagnostics.len();
        for (declared, param) in declared_params.iter().zip(params) {
            if let Some(annotation) = &declared.ty {
                let annotated = self.resolve_type(annotation);
                let _ = sub.unify_argument(param, &annotated, self.resolver());
            }
        }
        self.diagnostics.truncate(diagnostics_before);
    }

    fn function_literal_params(&self, expr: ExprId) -> Option<Vec<crate::ParamDecl>> {
        match &self.ast.expr(expr).kind {
            ExprKind::Paren(inner) => self.function_literal_params(*inner),
            ExprKind::FunctionExpression { function, .. } => {
                self.function_literal_params(*function)
            }
            ExprKind::Arrow { params, .. } => Some(params.clone()),
            _ => None,
        }
    }

    /// A function literal with a parameter left for its context to type,
    /// which is what TypeScript infers after the other arguments.
    fn is_context_sensitive_function(&self, expr: ExprId) -> bool {
        self.function_literal_params(expr)
            .is_some_and(|params| params.iter().any(|p| p.ty.is_none()))
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
    /// `signature_help` renders the callee for a mismatch diagnostic.
    fn infer_generic_arguments(
        &mut self,
        args: &[ExprId],
        params: &[Param],
        rest_elem_ty: &Type,
        sub: &mut TypeParamSubstitution,
        signature_help: impl Fn(&mut Self) -> String,
    ) -> Vec<ExprId> {
        let has_rest = params.last().is_some_and(|p| p.rest);
        let fixed_count = params.iter().take_while(|p| !p.rest).count();
        let mut typed_slots: Vec<Option<ExprId>> = vec![None; args.len()];
        for deferred_pass in [false, true] {
            for (i, &arg_id) in args.iter().enumerate() {
                if typed_slots[i].is_some() {
                    continue;
                }
                let param_ty = if i < fixed_count {
                    params[i].ty.clone()
                } else if has_rest {
                    rest_elem_ty.clone()
                } else {
                    Type::Error
                };
                let deferred = function_part(&param_ty).is_some()
                    && self.is_context_sensitive_function(arg_id);
                if deferred && !deferred_pass {
                    self.bind_from_annotated_params(arg_id, &param_ty, sub);
                }
                if deferred != deferred_pass {
                    continue;
                }
                let hint = sub.apply(&param_ty);
                let errors_before = self.error_count();
                let (typed_id, arg_ty) = self.infer_expr(arg_id, Some(&hint));
                typed_slots[i] = Some(typed_id);
                let missing_slot = i >= fixed_count && !has_rest;
                if missing_slot || matches!(arg_ty, Type::Error) {
                    continue;
                }
                // An error inferring the argument already covers a mismatch here.
                let already_reported = self.error_count() > errors_before;
                if let Err(error) = sub.unify_argument(&param_ty, &arg_ty, self.resolver()) {
                    self.unify_argument_error(
                        error,
                        sub,
                        (&param_ty, &arg_ty),
                        typed_id,
                        already_reported,
                        &signature_help,
                    );
                }
            }
        }
        typed_slots.into_iter().flatten().collect()
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
    ) {
        let arg_span = self.typed_ast.expr(arg).span;
        match error {
            UnifyError::Conflict { .. } if already_reported => {}
            UnifyError::Conflict { name, prev, new } => self.error(
                arg_span,
                format!(
                    "type parameter `{name}` already bound to `{prev}`, cannot bind to `{new}`"
                ),
            ),
            UnifyError::Mismatch { expected, got } => {
                if self.structural_member_unify(sub, param_ty, arg_ty) || already_reported {
                    return;
                }
                let mut help = vec![signature_help(self)];
                help.extend(super::type_diff::type_mismatch_help(&expected, &got));
                self.error_with_help(
                    arg_span,
                    format!("expected `{expected}`, got `{got}`"),
                    help,
                );
            }
        }
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
    ) -> (TypedExprKind, Type) {
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
                );
                sub.insert(name.clone(), resolved);
            }
        }

        if let Some(want) = expected.filter(|want| pins_type_parameters(want)) {
            // Mismatch surfaces later at the outer infer_expr site with a better span.
            let _ = sub.unify(&ret, want, self.resolver());
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
            match &params.last().unwrap().ty {
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
        let mut typed_args =
            self.infer_generic_arguments(&args, &params, &rest_elem_ty, &mut sub, signature_help);

        if has_rest || typed_args.len() < params.len() {
            self.typed_ast
                .record_authored_arguments(span, typed_args.clone());
        }
        if arity_ok {
            self.fill_omitted_defaults(&params, args.len(), span, &mut typed_args);
        }
        // Packed rest array has type T[] — a composite, not a bare TypeVar —
        // so the GenericArgument zip tags it is_generic: false.
        if arity_ok && has_rest {
            self.pack_rest_tail(fixed_count, rest_elem_ty.clone(), span, &mut typed_args);
        }

        // `session.get(key)` without a type argument keeps its pre-generic
        // meaning: an unchecked read of `unknown` the caller narrows with a
        // runtime-checked `as`. Only a *written* `<unknown>` is an error, so
        // bind the parameter here rather than letting it go unbound and trip
        // both the inference error and the erasure gate below.
        if checked_get && !type_args_written {
            bind_remaining(&mut sub, &generics, Type::Unknown);
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
            );
        }

        self.bind_leftover_type_parameters(&mut sub, &generics, &ret, expected, errors_before_args);
        if let Err(unbound) = sub.resolve_all(&generics) {
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
                vec![callee.explicit_arg_hint(&callee_ident.name, &generics, &sub)],
            );
        }

        self.check_inferred_void_arguments(&params, &ret, &sub, span);
        let result_ty = sub.apply(&ret);

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
            return (TypedExprKind::Null, Type::Error);
        }

        let mut llm_schema = None;
        if checked_llm {
            match self.llm_call_schema(&result_ty, type_args_written, span) {
                Ok(schema) => llm_schema = schema,
                Err(()) => return (TypedExprKind::Null, Type::Error),
            }
        }
        if let Some(schema) = &llm_schema {
            self.substitute_schema_argument(&params, &mut typed_args, schema, span);
        }

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
        let type_predicate = predicate.map(|p| {
            Box::new(crate::TypePredicate {
                parameter_index: p.parameter_index,
                asserted_type: sub.apply(&p.asserted_type),
            })
        });
        // A checked read must not keep `return_cast`: it is a representation
        // cast that tests nothing, and on a stored or missing `null` its
        // `ref.cast` traps uncatchably before any structural check could run.
        // The call node is left producing `unknown` and the checked `Cast`
        // wrapped around it does the verifying. Intercepting here rather than
        // at a callsite is what covers every import form, since all four
        // callers thread the package-export mangled name through unchanged.
        let runtime_args: Vec<_> = generics
            .iter()
            .map(|name| sub.apply(&Type::TypeVar(name.clone())))
            .collect();
        for arg in &runtime_args {
            self.record_runtime_type_test(arg);
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
        (call, result_ty)
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
) {
    if let Err(unbound) = sub.resolve_all(generics) {
        for name in unbound {
            sub.insert(name, fallback.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitute_typevars_replaces_top_level_typevar() {
        let mut bindings: BTreeMap<String, Type> = BTreeMap::new();
        bindings.insert("T".into(), Type::Number);
        let out = substitute_typevars(&Type::TypeVar("T".into()), &bindings);
        assert_eq!(out, Type::Number);
    }

    #[test]
    fn substitute_typevars_recurses_into_nested_constructors() {
        let mut bindings: BTreeMap<String, Type> = BTreeMap::new();
        bindings.insert("T".into(), Type::String);
        let arr = Type::Array(Box::new(Type::TypeVar("T".into())));
        let out = substitute_typevars(&arr, &bindings);
        assert_eq!(out, Type::Array(Box::new(Type::String)));
        let func = Type::Function {
            params: vec![Type::TypeVar("T".into())],
            ret: Box::new(Type::TypeVar("T".into())),
            predicate: None,
            has_rest: false,
        };
        let out = substitute_typevars(&func, &bindings);
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
        let out = substitute_typevars(&Type::TypeVar("U".into()), &bindings);
        assert_eq!(out, Type::TypeVar("U".into()));
    }

    use super::super::test_support::{run, run_clean};
    use crate::{ExprId, PackageDeclaration, Param, StmtId, TypedAst, TypedStmtKind, ValueKind};

    fn last_call_resolved_ty(ta: &TypedAst) -> Type {
        for &id in ta.top_level_statements.iter().rev() {
            if let TypedStmtKind::AssignGlobal { value, .. } = &ta.stmt(id).kind {
                return ta.expr(*value).ty.clone();
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
        let stmt = ta.stmt(stmt_id);
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
        let expr = ta.expr(expr_id);
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
        let result = sub.resolve_all(&["T".to_string()]);
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
        let TypedStmtKind::Block(stmts) = &ta.stmt(body).kind else {
            panic!("expected block body");
        };
        let TypedStmtKind::Const { ty, .. } = &ta.stmt(stmts[0]).kind else {
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
        let TypedStmtKind::Block(stmts) = &ta.stmt(body).kind else {
            panic!("expected block body");
        };
        let TypedStmtKind::Const { ty, .. } = &ta.stmt(stmts[0]).kind else {
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
        let TypedStmtKind::Block(stmts) = &ta.stmt(body).kind else {
            panic!("expected block body");
        };
        let TypedStmtKind::Const { ty, .. } = &ta.stmt(stmts[0]).kind else {
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
        let TypedStmtKind::Block(stmts) = &ta.stmt(body).kind else {
            panic!("expected block body");
        };
        let TypedStmtKind::Let { ty, .. } = &ta.stmt(stmts[1]).kind else {
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
    fn under_constrained_method_generic_still_diagnoses() {
        let (_, diags) = run(r#"
            interface Box<T> {
                empty<U>(): U[];
            }
            function go(b: Box<number>): void {
                b.empty();
            }
            "#);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("cannot infer type parameter")
                    && d.message.contains("`U`")),
            "expected under-constrained diagnostic, got: {diags:?}",
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
