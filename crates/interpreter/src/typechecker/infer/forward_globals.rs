//! A module-level `let`/`const` used in a function body above its declaration.
//!
//! The top level is inferred in source order, so a later `let`/`const` has no
//! binding yet when an arrow or function expression above it is inferred. A
//! function body only runs when called, so, as in TypeScript, it may name one.
//! Its binding is made early, at the first such use, from what its declaration
//! states without inferring its initializer: a type annotation, a literal, or an
//! arrow whose parameter and return types are all written. Code that runs
//! directly at the top level still can't name it before its declaration.

use crate::compiler_error::CompilerFailure;

use crate::{ExprId, ExprKind, Ident, Span, StmtId, StmtKind, Type, TypeAnnotation, ValueKind};

use super::Inferer;
use super::void_value::ValuePosition;

/// A module-level `let`/`const` that step 2 has not reached yet.
pub(in crate::typechecker) enum LaterGlobal {
    /// No function body has used it.
    Pending(StmtId),
    /// A function body used it, and it was bound with `ty`.
    BoundEarly(EarlyBinding),
}

pub(in crate::typechecker) struct EarlyBinding {
    ty: Type,
    /// Where it was first used, when no type could be bound there: reported at
    /// the declaration, which is inferred once, unlike a use in a loop condition
    /// whose diagnostics are discarded and inferred again.
    untyped_use: Option<(Span, Untyped)>,
}

/// Why a later declaration had no type at its first use.
#[derive(Clone, Copy)]
enum Untyped {
    /// The declaration states no type.
    Unstated,
    /// Its annotation names something declared after that use.
    AnnotationUnresolved,
}

impl Inferer<'_> {
    /// Record the module-level `let`/`const` declarations that step 2 has yet to
    /// reach. A name something else already declares keeps that binding.
    pub(super) fn collect_later_globals(&mut self) -> Result<(), CompilerFailure> {
        self.later_globals.clear();
        for &stmt in &self.ast.top_level {
            let (StmtKind::Let { name, .. } | StmtKind::Const { name, .. }) =
                &self.ast.try_stmt(stmt).map_err(super::arena_failure)?.kind
            else {
                continue;
            };
            if !self.top_symbols.contains_key(&name.name) {
                self.later_globals
                    .entry(name.name.clone())
                    .or_insert(LaterGlobal::Pending(stmt));
            }
        }
        Ok(())
    }

    /// Whether `name` names a later `let`/`const` that code running directly at
    /// the top level can't see yet. Used in a function body instead, the later
    /// declaration is bound early here, so a lookup finds it.
    pub(super) fn hides_later_global(
        &mut self,
        name: &str,
        span: Span,
    ) -> Result<bool, CompilerFailure> {
        let Some(later) = self.later_globals.get(name) else {
            return Ok(false);
        };
        if !self.in_nested_function {
            return Ok(true);
        }
        if let LaterGlobal::Pending(stmt) = *later {
            self.bind_global_early(stmt, span)?;
        }
        Ok(false)
    }

    /// Whether `name` names a module-level `let`/`const` not declared yet.
    pub(super) fn is_later_global(&self, name: &str) -> bool {
        self.later_globals.contains_key(name)
    }

    /// Step 2 has reached the declaration of `name`, bound with type `bound`:
    /// drop its early binding, and report a use above it the binding couldn't
    /// serve. `declaration_failed` says the declaration reported an error itself.
    pub(super) fn finish_later_global(
        &mut self,
        name: &Ident,
        bound: &Type,
        declaration_failed: bool,
    ) {
        let Some(LaterGlobal::BoundEarly(early)) = self.forget_later_global(&name.name) else {
            return;
        };
        if let Some((use_span, why)) = early.untyped_use {
            // An annotation that fails at the declaration too is reported there.
            let declaration_reports_it =
                matches!(why, Untyped::AnnotationUnresolved) && declaration_failed;
            if !declaration_reports_it {
                self.report_untyped_later_global(name, use_span, why);
            }
            return;
        }
        if early.ty == *bound || matches!(early.ty, Type::Error) || matches!(bound, Type::Error) {
            return;
        }
        self.error_with_help(
            name.span,
            format!(
                "`{}` is used above its declaration as `{}`, but its type is `{bound}`",
                name.name, early.ty
            ),
            vec![format!("annotate its type: `{}: {bound}`", name.name)],
        );
    }

    /// Stop treating `name` as a later global, removing an early binding.
    pub(super) fn forget_later_global(&mut self, name: &str) -> Option<LaterGlobal> {
        let later = self.later_globals.remove(name)?;
        if matches!(later, LaterGlobal::BoundEarly(_)) {
            self.top_symbols.remove(name);
        }
        Some(later)
    }

    fn report_untyped_later_global(&mut self, name: &Ident, use_span: Span, why: Untyped) {
        let (message, fix) = match why {
            Untyped::Unstated => (
                "so its type must be written",
                format!("annotate its type: `{}: T = …`", name.name),
            ),
            Untyped::AnnotationUnresolved => (
                "so its type can't name anything declared below that function",
                "write that type out instead".to_string(),
            ),
        };
        self.error_with_help_and_notes(
            name.span,
            format!(
                "`{}` is used by a function above its declaration, {message}",
                name.name
            ),
            vec![
                fix,
                "or move the declaration above the function that uses it".to_string(),
            ],
            vec![(use_span, format!("`{}` is used here", name.name))],
        );
    }

    fn bind_global_early(&mut self, stmt: StmtId, use_span: Span) -> Result<(), CompilerFailure> {
        let (name, annotation, value, is_const) =
            match &self.ast.try_stmt(stmt).map_err(super::arena_failure)?.kind {
                StmtKind::Let {
                    name, ty, value, ..
                } => (name.clone(), ty.clone(), *value, false),
                StmtKind::Const {
                    name, ty, value, ..
                } => (name.clone(), ty.clone(), *value, true),
                _ => {
                    return Err(super::inference_failure(
                        "later global is not a let or const",
                    ));
                }
            };
        // The declaration is at module level, so its annotation must not see
        // the locals around the use.
        let use_scopes = std::mem::take(&mut self.scopes);
        let diagnostics_before = self.diagnostics.len();
        let stated = self.stated_global_type(annotation.as_ref(), value, is_const);
        self.scopes = use_scopes;
        // An annotation that doesn't resolve here either names something below
        // the use or is wrong; the declaration reports it.
        let annotation_failed = self.diagnostics.len() != diagnostics_before;
        self.diagnostics.truncate(diagnostics_before);
        let early = match stated? {
            Some(ty) if !annotation_failed => EarlyBinding {
                ty,
                untyped_use: None,
            },
            Some(_) => EarlyBinding {
                ty: Type::Error,
                untyped_use: Some((use_span, Untyped::AnnotationUnresolved)),
            },
            None => EarlyBinding {
                ty: Type::Error,
                untyped_use: Some((use_span, Untyped::Unstated)),
            },
        };
        let kind = if is_const {
            ValueKind::Const {
                ty: early.ty.clone(),
                doc: None,
            }
        } else {
            ValueKind::Let {
                ty: early.ty.clone(),
                doc: None,
            }
        };
        self.bind_top(&name, kind)?;
        self.later_globals
            .insert(name.name, LaterGlobal::BoundEarly(early));
        Ok(())
    }

    /// The type a declaration states without its initializer being inferred,
    /// matching what step 2 binds: the annotation, a literal (widened for a
    /// `let`), or an arrow with every parameter and its return type written.
    fn stated_global_type(
        &mut self,
        annotation: Option<&TypeAnnotation>,
        value: ExprId,
        is_const: bool,
    ) -> Result<Option<Type>, CompilerFailure> {
        if let Some(annotation) = annotation {
            return self.resolve_type(annotation).map(Some);
        }
        if let Some(literal) = super::stmt::literal_type_of(self.ast, value)? {
            return Ok(Some(if is_const {
                literal
            } else {
                literal.widen_literal()
            }));
        }
        self.written_arrow_type(value)
    }

    fn written_arrow_type(&mut self, value: ExprId) -> Result<Option<Type>, CompilerFailure> {
        let arrow = match &self.ast.try_expr(value).map_err(super::arena_failure)?.kind {
            ExprKind::Paren(inner) => return self.written_arrow_type(*inner),
            ExprKind::FunctionExpression {
                this_type: None,
                function,
                ..
            } => *function,
            ExprKind::Arrow { .. } => value,
            _ => return Ok(None),
        };
        let ExprKind::Arrow {
            params,
            return_type: Some(return_type),
            type_predicate: None,
            ..
        } = &self.ast.try_expr(arrow).map_err(super::arena_failure)?.kind
        else {
            return Ok(None);
        };
        let mut param_types = Vec::with_capacity(params.len());
        for param in params {
            let (Some(annotation), None) = (&param.ty, param.default) else {
                return Ok(None);
            };
            param_types.push(self.resolve_value_type(annotation, ValuePosition::Parameter)?);
        }
        Ok(Some(Type::Function {
            params: param_types,
            ret: Box::new(self.resolve_type(return_type)?),
            predicate: None,
            has_rest: params.last().is_some_and(|p| p.rest),
        }))
    }
}
