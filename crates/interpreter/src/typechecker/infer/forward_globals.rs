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
    /// A function body used it, so it was bound early.
    BoundEarly(EarlyBinding),
}

/// A later declaration's binding, made at its first use from a function body.
pub(in crate::typechecker) struct EarlyBinding {
    stmt: StmtId,
    ty: Type,
    /// Where it was first used, when no type could be bound there: reported at
    /// the declaration, which is inferred once, unlike a use in a loop condition
    /// whose diagnostics are discarded and inferred again.
    untyped_use: Option<(Span, Untyped)>,
}

/// Why a later declaration's type couldn't be bound at its first use.
#[derive(Clone, Copy)]
enum Untyped {
    /// The declaration states no type: it would need its initializer inferred.
    Unstated,
    /// Its written type doesn't resolve there: it names something declared
    /// after that use, or it is wrong.
    AnnotationUnresolved,
}

/// The parts of a module-level `let`/`const` declaration.
struct LaterDeclaration {
    name: Ident,
    annotation: Option<TypeAnnotation>,
    value: ExprId,
    is_const: bool,
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
    /// serve.
    pub(super) fn finish_later_global(
        &mut self,
        name: &Ident,
        bound: &Type,
    ) -> Result<(), CompilerFailure> {
        let Some(LaterGlobal::BoundEarly(early)) = self.forget_later_global(&name.name) else {
            return Ok(());
        };
        if let Some((use_span, why)) = early.untyped_use {
            // A written type that is wrong here too is reported by the declaration.
            if matches!(why, Untyped::Unstated) || self.stated_type_resolves(early.stmt)? {
                self.report_untyped_later_global(name, use_span, why);
            }
            return Ok(());
        }
        if early.ty == *bound || matches!(early.ty, Type::Error) || matches!(bound, Type::Error) {
            return Ok(());
        }
        self.error_with_help(
            name.span,
            format!(
                "`{}` is used above its declaration as `{}`, but its type is `{bound}`",
                name.name, early.ty
            ),
            vec![format!("annotate its type: `{}: {bound}`", name.name)],
        );
        Ok(())
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

    /// Whether the declaration's stated type resolves now, at the declaration.
    /// One that doesn't is reported there, so its earlier use needs no error.
    fn stated_type_resolves(&mut self, stmt: StmtId) -> Result<bool, CompilerFailure> {
        let declaration = self.later_declaration(stmt)?;
        Ok(self.resolve_stated_type(&declaration)?.is_ok())
    }

    fn bind_global_early(&mut self, stmt: StmtId, use_span: Span) -> Result<(), CompilerFailure> {
        let declaration = self.later_declaration(stmt)?;
        let (ty, untyped_use) = match self.resolve_stated_type(&declaration)? {
            Ok(ty) => (ty, None),
            Err(why) => (Type::Error, Some((use_span, why))),
        };
        let ty = if declaration.is_const {
            ty
        } else {
            self.global_storage_ty(&declaration.name.name, ty)
        };
        let kind = if declaration.is_const {
            ValueKind::Const {
                ty: ty.clone(),
                doc: None,
            }
        } else {
            ValueKind::Let {
                ty: ty.clone(),
                doc: None,
            }
        };
        self.bind_top(&declaration.name, kind)?;
        self.later_globals.insert(
            declaration.name.name,
            LaterGlobal::BoundEarly(EarlyBinding {
                stmt,
                ty,
                untyped_use,
            }),
        );
        Ok(())
    }

    fn later_declaration(&self, stmt: StmtId) -> Result<LaterDeclaration, CompilerFailure> {
        let (name, annotation, value, is_const) =
            match &self.ast.try_stmt(stmt).map_err(super::arena_failure)?.kind {
                StmtKind::Let {
                    name, ty, value, ..
                } => (name, ty, *value, false),
                StmtKind::Const {
                    name, ty, value, ..
                } => (name, ty, *value, true),
                _ => {
                    return Err(super::inference_failure(
                        "later global is not a let or const",
                    ));
                }
            };
        Ok(LaterDeclaration {
            name: name.clone(),
            annotation: annotation.clone(),
            value,
            is_const,
        })
    }

    /// The type the declaration states, resolved at module level: the locals
    /// around a use can't name it. Any diagnostic is discarded, since the
    /// declaration reports it when reached.
    fn resolve_stated_type(
        &mut self,
        declaration: &LaterDeclaration,
    ) -> Result<Result<Type, Untyped>, CompilerFailure> {
        let use_scopes = std::mem::take(&mut self.scopes);
        let errors_before = self.error_count();
        let diagnostics_before = self.diagnostics.len();
        let stated = self.stated_global_type(declaration);
        self.scopes = use_scopes;
        let reported_error = self.error_count() != errors_before;
        self.diagnostics.truncate(diagnostics_before);
        Ok(match stated? {
            Some(ty) if !reported_error => Ok(ty),
            Some(_) => Err(Untyped::AnnotationUnresolved),
            None => Err(Untyped::Unstated),
        })
    }

    /// The type a declaration states without its initializer being inferred,
    /// matching what step 2 binds: the annotation, a literal (widened for a
    /// `let`), or an arrow with every parameter and its return type written.
    fn stated_global_type(
        &mut self,
        declaration: &LaterDeclaration,
    ) -> Result<Option<Type>, CompilerFailure> {
        if let Some(annotation) = &declaration.annotation {
            return self.resolve_type(annotation).map(Some);
        }
        let value = declaration.value;
        if let Some(literal) = stated_literal_type(self.ast, value)? {
            return Ok(Some(if declaration.is_const {
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

/// The literal type an initializer states without being inferred: a literal
/// token through any parentheses, or a `?:` whose branches are literals of one
/// primitive type. Any other initializer states nothing.
fn stated_literal_type(ast: &crate::Ast, value: ExprId) -> Result<Option<Type>, CompilerFailure> {
    Ok(
        match &ast.try_expr(value).map_err(super::arena_failure)?.kind {
            ExprKind::Number(v) => Some(Type::NumberLiteral(crate::types::LiteralF64(*v))),
            ExprKind::String(s) => Some(Type::StringLiteral(s.clone())),
            ExprKind::Boolean(b) => Some(Type::BooleanLiteral(*b)),
            ExprKind::Paren(inner) => stated_literal_type(ast, *inner)?,
            ExprKind::Ternary { then_, else_, .. } => {
                match (
                    stated_literal_type(ast, *then_)?,
                    stated_literal_type(ast, *else_)?,
                ) {
                    (Some(then_ty), Some(else_ty))
                        if then_ty.widen_literal() == else_ty.widen_literal() =>
                    {
                        Some(Type::union(vec![then_ty, else_ty]))
                    }
                    _ => None,
                }
            }
            _ => None,
        },
    )
}
