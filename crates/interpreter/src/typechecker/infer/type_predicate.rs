//! User-defined type guard machinery.

use crate::{ExprId, Param, Type, TypedExpr, types::ObjectField};

use super::{Inferer, assignable, narrowing};

fn compose_param_narrowing(
    env: &narrowing::NarrowEnv,
    path: &narrowing::ReferencePath,
    param_ty: &Type,
) -> Type {
    let base = env
        .get(path)
        .map_or_else(|| param_ty.clone(), |view| view.narrowed_ty.clone());
    let Type::Object { fields } = base.peel().clone() else {
        return base;
    };
    let mut refined = fields;
    for (entry_path, view) in env {
        if entry_path.root != path.root {
            continue;
        }
        if entry_path.chain.len() != 1 {
            continue;
        }
        let narrowing::PathElem::Field(field_name) = &entry_path.chain[0] else {
            continue;
        };
        match refined.get_mut(field_name) {
            Some(existing) => existing.ty = view.narrowed_ty.clone(),
            None => {
                refined.insert(
                    field_name.clone(),
                    ObjectField::required(view.narrowed_ty.clone()),
                );
            }
        }
    }
    Type::Object { fields: refined }
}

impl Inferer<'_> {
    pub(super) fn resolve_type_predicate(
        &mut self,
        pred: &crate::TypePredicateAnnotation,
        params: &[Param],
    ) -> Option<crate::TypePredicate> {
        let idx = params.iter().position(|p| p.name == pred.param.name);
        let Some(idx) = idx else {
            self.error(
                pred.param.span,
                format!(
                    "type predicate references unknown parameter `{}`",
                    pred.param.name,
                ),
            );
            return None;
        };
        let asserted_type = self.resolve_type(&pred.asserted);
        if matches!(asserted_type, Type::Error) {
            return None;
        }
        Some(crate::TypePredicate {
            parameter_index: idx as u32,
            asserted_type,
        })
    }

    pub(super) fn predicate_envs_user_guard(
        &mut self,
        predicate: &crate::TypePredicate,
        args: &[ExprId],
    ) -> Option<(narrowing::NarrowEnv, narrowing::NarrowEnv)> {
        let idx = predicate.parameter_index as usize;
        let arg_id = *args.get(idx)?;
        self.narrow_path_to_asserted(arg_id, &predicate.asserted_type)
    }

    /// Build the (true, false) narrow envs for refining the binding referenced by `target`
    /// to `asserted`. Shared by user-defined `s is T` guards and `x instanceof Foo`: the
    /// true branch keeps the union members assignable to `asserted` (collapsing to
    /// `asserted` for a proven subtype), the false branch drops them. Returns empty envs
    /// when `target` isn't a narrowable reference path, and a `Type::Error` narrowed type
    /// (skipped by `wrap_narrow_regions`) when the predicate eliminates every member.
    pub(super) fn narrow_path_to_asserted(
        &mut self,
        target: ExprId,
        asserted: &Type,
    ) -> Option<(narrowing::NarrowEnv, narrowing::NarrowEnv)> {
        let arg_expr = self.typed_ast.expr(target);
        let path = self.expr_to_reference_path(arg_expr)?;
        if self.path_root_is_captured_mutator(&path) {
            return Some((narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new()));
        }
        let from_ty = arg_expr.ty.clone();
        let arg_span = arg_expr.span;
        let fallback_kind = arg_expr.kind.clone();

        // prefer un-narrowed source so NarrowRegion materialization doesn't depend on a chain-only shadow binding.
        let arg_kind = self
            .synthesize_unnarrowed_source(&path, arg_span)
            .unwrap_or(fallback_kind);

        // Intersect to keep element-type precision: Array.isArray(x: number[] | string) → number[], not Array<unknown>.
        let true_ty = match from_ty.peel() {
            Type::Union(members) => {
                let keep: Vec<Type> = members
                    .iter()
                    .filter(|m| assignable(m, asserted, self.resolver()))
                    .cloned()
                    .collect();
                if keep.is_empty() {
                    if assignable(asserted, from_ty.peel(), self.resolver()) {
                        asserted.clone()
                    } else {
                        Type::Error
                    }
                } else {
                    Type::union(keep)
                }
            }
            ty if assignable(ty, asserted, self.resolver()) => ty.clone(),
            ty if assignable(asserted, ty, self.resolver()) => asserted.clone(),
            _ => Type::Error,
        };
        let false_ty = match from_ty.peel() {
            Type::Union(members) => {
                let keep: Vec<Type> = members
                    .iter()
                    .filter(|m| !assignable(m, asserted, self.resolver()))
                    .cloned()
                    .collect();
                Type::union(keep)
            }
            ty if assignable(ty, asserted, self.resolver()) => Type::Error,
            ty => ty.clone(),
        };

        let source_true = self.typed_ast.push_expr(TypedExpr {
            kind: arg_kind.clone(),
            span: arg_span,
            ty: from_ty.clone(),
        });
        let source_false = self.typed_ast.push_expr(TypedExpr {
            kind: arg_kind,
            span: arg_span,
            ty: from_ty,
        });
        let mut true_env = narrowing::NarrowEnv::new();
        let mut false_env = narrowing::NarrowEnv::new();
        true_env.insert(
            path.clone(),
            narrowing::NarrowedView {
                narrowed_ty: true_ty,
                facts: narrowing::TypeFacts::EMPTY,
                excluded_literals: std::collections::BTreeSet::new(),
                binding: self.mint_narrow_binding(arg_span),
                source: source_true,
            },
        );
        false_env.insert(
            path,
            narrowing::NarrowedView {
                narrowed_ty: false_ty,
                facts: narrowing::TypeFacts::EMPTY,
                excluded_literals: std::collections::BTreeSet::new(),
                binding: self.mint_narrow_binding(arg_span),
                source: source_false,
            },
        );
        Some((true_env, false_env))
    }

    pub(super) fn validate_type_predicate_return(&mut self, value_id: ExprId, span: crate::Span) {
        let Some((predicate, param_name)) = self.current_type_predicate.clone() else {
            return;
        };
        let Some(scope_entry) = self.scopes.get(&param_name).cloned() else {
            return;
        };
        // x is T only attaches on true returns; return false makes no narrowing claim.
        if matches!(
            self.typed_ast.expr(value_id).kind,
            crate::TypedExprKind::Boolean(false)
        ) {
            return;
        }
        let path = narrowing::ReferencePath::root(narrowing::BindingId::Local {
            name: param_name.clone(),
            decl_scope: scope_entry.decl_scope,
        });
        let (return_true_env, _return_false_env) = self.predicate_envs(value_id);
        // Merge enclosing if-branch narrowings (narrow_scopes) with the return expr's env;
        // outer-frames-first so the return value's env is the final (innermost) override.
        let mut combined_env = narrowing::NarrowEnv::new();
        for frame in &self.narrow_scopes {
            for (p, v) in frame {
                combined_env.insert(p.clone(), v.clone());
            }
        }
        for (p, v) in return_true_env {
            combined_env.insert(p, v);
        }
        // Compose root + field-path narrowings: in `if (x !== null && "f" in x && typeof x.f === "T")`,
        // neither x→Object{f:unknown} nor x.f→T alone matches the asserted shape, but together they do.
        let narrowed_ty = compose_param_narrowing(&combined_env, &path, &scope_entry.ty);
        // Substitute TypeVars → body GenericParams; without this, assignable treats TypeVar as a wildcard
        // and generic guards like `f<T>(x: T | null): x is T` slip through validation.
        let asserted_body = match self.body_instantiations.last() {
            Some(frame) if !frame.is_empty() => {
                super::generic::substitute_typevars(&predicate.asserted_type, frame)
            }
            _ => predicate.asserted_type.clone(),
        };
        if !assignable(&narrowed_ty, &asserted_body, self.resolver()) {
            self.error_with_help(
                span,
                format!(
                    "type-guard return value does not narrow parameter `{}` to `{}`",
                    param_name, predicate.asserted_type,
                ),
                vec![format!(
                    "at this return, `{}` is inferred as `{}`",
                    param_name, narrowed_ty,
                )],
            );
        }
    }
}
