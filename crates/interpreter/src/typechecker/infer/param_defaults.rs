//! Parameter defaults run in the callee, before its body and in source order.

use crate::compiler_error::CompilerFailure;
use crate::{ExprId, ParamDecl, Span, StmtId, Type, TypedExprKind, TypedStmt, TypedStmtKind};

use super::literal_freshness::LiteralOrigin;
use super::{Inferer, arena_failure};

impl Inferer<'_> {
    /// Bind parameters and lower defaults to assignments to their incoming slots.
    /// Reads coerce from ABI storage to the initialized binding's type.
    /// Call after pushing the parameter scope and before inferring the body.
    pub(super) fn infer_parameter_defaults(
        &mut self,
        params: &[ParamDecl],
        input_types: &[Type],
    ) -> Result<Vec<StmtId>, CompilerFailure> {
        if params.len() != input_types.len() {
            return Err(super::inference_failure(
                "parameter/input type length mismatch",
            ));
        }
        // All parameters share their incoming slots, including references from
        // closures created by an earlier default. Lexical analysis rejects reads
        // before initialization while permitting deferred closure reads.
        for (param, input_ty) in params.iter().zip(input_types) {
            let write_ty = self.parameter_write_type(param, input_ty)?;
            let body_ty = if param.default.is_some() {
                without_undefined(input_ty)
            } else {
                input_ty.clone()
            };
            self.bind_initialized_parameter(param, input_ty, body_ty, write_ty);
        }
        // Without defaults or patterns no code runs before the body, so every
        // parameter is initialized on entry and needs no prologue.
        if !params.iter().any(|param| {
            param.default.is_some() || self.ast.parameter_bindings.contains_key(&param.name.span)
        }) {
            for (param, input_ty) in params.iter().zip(input_types) {
                self.record_authored_parameter_type(param.name.span, input_ty);
            }
            return Ok(Vec::new());
        }
        self.predeclare_parameter_destructuring(params)?;
        if !self.prebinding_parameter_types && params.iter().any(|param| param.default.is_some()) {
            // A default may name any parameter, so every one is declared with
            // its initialized type first. That changes what pattern bindings
            // decompose, so they are predeclared again on top of it.
            self.predeclare_initialized_parameters(params, input_types)?;
            self.predeclare_parameter_destructuring(params)?;
        }
        let mut prologue = Vec::new();
        for (param, input_ty) in params.iter().zip(input_types) {
            if let Some(default) = param.default {
                self.lower_parameter_default(param, input_ty, default, &mut prologue)?;
            } else {
                let incoming = self.parameter_input(param, input_ty)?;
                prologue.push(self.initialize_parameter(param, input_ty, incoming)?);
                self.record_authored_parameter_type(param.name.span, input_ty);
            }
            self.infer_parameter_destructuring(param, &mut prologue)?;
        }
        Ok(prologue)
    }

    /// Initialize `param` from its default when the incoming value is
    /// `undefined`, and from the incoming value otherwise.
    fn lower_parameter_default(
        &mut self,
        param: &ParamDecl,
        input_ty: &Type,
        default: ExprId,
        prologue: &mut Vec<StmtId>,
    ) -> Result<(), CompilerFailure> {
        let span = param.name.span;
        let target = without_undefined(input_ty);
        let declared = if let Some(annotation) = &param.ty {
            self.resolve_type(annotation)?
        } else {
            input_ty.clone()
        };
        let (default_value, default_ty) = self.infer_expr(default, Some(&declared))?;
        let default_span = self.ast.try_expr(default).map_err(arena_failure)?.span;
        // Replace only the outer contextual mismatch with the parameter
        // diagnostic; keep errors from inside the initializer intact.
        self.drop_contextual_mismatch(default_span, &declared, &default_ty);
        self.check_parameter_default(param, &declared, &default_ty, default_span);
        let body_ty = self.initialized_parameter_type(input_ty, &default_ty);
        let write_ty = self.parameter_write_type(param, input_ty)?;
        self.record_authored_parameter_type(span, &write_ty);
        let incoming = self.parameter_input(param, input_ty)?;
        let undefined =
            self.push_synthetic_expr(TypedExprKind::Undefined, Type::Undefined, span)?;
        let condition = self.push_synthetic_expr(
            TypedExprKind::Binary {
                op: crate::ast::BinOp::Eq,
                lhs: incoming,
                rhs: undefined,
            },
            Type::Boolean,
            span,
        )?;
        let present = if matches!(target.peel(), Type::Never) {
            undefined
        } else {
            self.push_synthetic_expr(
                TypedExprKind::Cast {
                    value: incoming,
                    target_ty: target.clone(),
                    check: None,
                },
                target,
                span,
            )?
        };
        let value = self.push_synthetic_expr(
            TypedExprKind::Ternary {
                cond: condition,
                then_: default_value,
                else_: present,
            },
            body_ty.clone(),
            span,
        )?;
        prologue.push(self.initialize_parameter(param, input_ty, value)?);
        self.bind_initialized_parameter(param, input_ty, body_ty, write_ty);
        Ok(())
    }

    fn parameter_write_type(
        &mut self,
        param: &ParamDecl,
        input_ty: &Type,
    ) -> Result<Type, CompilerFailure> {
        if param.default.is_none() {
            return Ok(input_ty.clone());
        }
        if let Some(annotation) = &param.ty {
            return self.resolve_type(annotation);
        }
        let span = param.name.span;
        let source_ty = self
            .typed_ast
            .authored_parameter_types
            .get(&(span.file.0, span.start, span.end))
            .ok_or_else(|| {
                super::inference_failure("missing inferred parameter source type").with_span(span)
            })?;
        self.apply_body_instantiations(source_ty)
            .map_err(super::type_limit_at(span))
    }

    fn initialized_parameter_type(&self, input_ty: &Type, default_ty: &Type) -> Type {
        let present = without_undefined(input_ty);
        if super::assignable::assignable(default_ty, &present, self.resolver()) {
            present
        } else {
            Type::union(vec![present, default_ty.widen_literal()])
        }
    }

    fn bind_initialized_parameter(
        &mut self,
        param: &ParamDecl,
        input_ty: &Type,
        body_ty: Type,
        write_ty: Type,
    ) {
        let write_ty = self.local_storage_ty(&param.name, write_ty);
        let read_ty = if self
            .captured_mutators
            .contains(&(param.name.name.clone(), param.name.span))
        {
            write_ty.clone()
        } else {
            self.local_storage_ty(&param.name, body_ty)
        };
        // An annotated parameter's literal types are regular. One typed only by
        // the expected function type takes whatever literals that type was
        // inferred with, so its literal types count as fresh.
        let literal_origin = if param.ty.is_some() {
            LiteralOrigin::Declared
        } else {
            LiteralOrigin::Unknown
        };
        self.scopes.insert_parameter(
            param.name.name.clone(),
            read_ty,
            input_ty.clone(),
            param.name.span,
            literal_origin,
        );
        self.scopes
            .set_parameter_declared_type(&param.name.name, write_ty);
    }

    /// Defaults can create closures that read later parameters. Discover their
    /// initialized types before inferring those closure bodies; source input
    /// types include omission and do not describe the initialized reads.
    fn predeclare_initialized_parameters(
        &mut self,
        params: &[ParamDecl],
        input_types: &[Type],
    ) -> Result<(), CompilerFailure> {
        let types = self.with_parameter_type_trial(|this| {
            this.initialized_parameter_types(params, input_types)
        })?;
        for ((param, input_ty), (mut body_ty, write_ty)) in
            params.iter().zip(input_types).zip(types)
        {
            // An earlier default's closure can run after a body assignment.
            // The ordered pass restores the initial read refinement for the
            // parameter's own body until that assignment executes.
            if self.last_assignments.contains_key(&param.name.span) {
                body_ty = write_ty.clone();
            }
            self.bind_initialized_parameter(param, input_ty, body_ty, write_ty);
        }
        Ok(())
    }

    fn initialized_parameter_types(
        &mut self,
        params: &[ParamDecl],
        input_types: &[Type],
    ) -> Result<Vec<(Type, Type)>, CompilerFailure> {
        let mut types = Vec::with_capacity(params.len());
        for (param, input_ty) in params.iter().zip(input_types) {
            let write_ty = self.parameter_write_type(param, input_ty)?;
            let body_ty = if let Some(default) = param.default {
                let kind = &self.ast.try_expr(default).map_err(arena_failure)?.kind;
                if matches!(
                    kind,
                    crate::ExprKind::Arrow { .. } | crate::ExprKind::FunctionExpression { .. }
                ) {
                    // Constructing a function always yields a defined value;
                    // its body is checked once all sibling types are ready.
                    without_undefined(input_ty)
                } else {
                    let (_, default_ty) = self.infer_expr(default, Some(&write_ty))?;
                    self.initialized_parameter_type(input_ty, &default_ty)
                }
            } else {
                input_ty.clone()
            };
            self.bind_initialized_parameter(param, input_ty, body_ty.clone(), write_ty.clone());
            self.infer_parameter_destructuring(param, &mut Vec::new())?;
            types.push((body_ty, write_ty));
        }
        Ok(types)
    }

    /// Discover later pattern bindings before an earlier default closure reads
    /// them. The trial has no runtime roots; the ordered pass below remains the
    /// sole source of initialization statements and user-facing diagnostics.
    fn predeclare_parameter_destructuring(
        &mut self,
        params: &[ParamDecl],
    ) -> Result<(), CompilerFailure> {
        let statements = params
            .iter()
            .filter_map(|param| self.ast.parameter_bindings.get(&param.name.span))
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        if statements.is_empty() {
            return Ok(());
        }
        let bindings =
            self.with_parameter_type_trial(|this| this.infer_parameter_binding_types(&statements))?;
        for (name, ty, is_const) in bindings {
            self.scopes.insert(name.name, ty, is_const, name.span);
        }
        Ok(())
    }

    /// Keep speculative flow and parameter metadata out of the ordered pass.
    /// Arena work and type-budget charges remain accounted for; nested trials
    /// are disabled so nested default expressions do not multiply the work.
    fn with_parameter_type_trial<T>(
        &mut self,
        infer: impl FnOnce(&mut Self) -> Result<T, CompilerFailure>,
    ) -> Result<T, CompilerFailure> {
        let scopes = self.scopes.clone();
        let reachable = self.reachable;
        let diagnostics = self.diagnostics.len();
        let pattern_sources = self.pattern_sources.clone();
        let last_write_spans = self.last_write_spans.clone();
        let nested_functions = self.nested_functions.clone();
        let inputs = self.typed_ast.parameter_inputs.clone();
        let initializations = self.typed_ast.parameter_initializations.clone();
        let prologues = self.typed_ast.parameter_default_prologues.clone();
        let authored_types = self.typed_ast.authored_parameter_types.clone();
        self.enter_function_declaration_narrow_boundary();
        let was_prebinding = std::mem::replace(&mut self.prebinding_parameter_types, true);
        let result = infer(self);
        self.prebinding_parameter_types = was_prebinding;
        let restored = self.exit_closure_narrow_boundary();
        self.scopes = scopes;
        self.reachable = reachable;
        self.pattern_sources = pattern_sources;
        self.last_write_spans = last_write_spans;
        self.nested_functions = nested_functions;
        self.typed_ast.parameter_inputs = inputs;
        self.typed_ast.parameter_initializations = initializations;
        self.typed_ast.parameter_default_prologues = prologues;
        self.typed_ast.authored_parameter_types = authored_types;
        let types = result?;
        restored?;
        self.diagnostics.truncate(diagnostics);
        Ok(types)
    }

    fn infer_parameter_binding_types(
        &mut self,
        statements: &[StmtId],
    ) -> Result<Vec<(crate::Ident, Type, bool)>, CompilerFailure> {
        let mut bindings = Vec::new();
        for &statement in statements {
            let (name, is_const) = match &self.ast.try_stmt(statement).map_err(arena_failure)?.kind
            {
                crate::StmtKind::Let { name, .. } => (name.clone(), false),
                crate::StmtKind::Const { name, .. } => (name.clone(), true),
                crate::StmtKind::ObjectRest { name, is_const, .. } => (name.clone(), *is_const),
                _ => {
                    return Err(super::inference_failure(
                        "parameter decomposition is not a binding",
                    ));
                }
            };
            self.infer_parameter_binding_stmt(statement)?;
            let ty = self
                .scopes
                .get(&name.name)
                .ok_or_else(|| {
                    super::inference_failure("parameter pattern binding was not inferred")
                        .with_span(name.span)
                })?
                .ty
                .clone();
            bindings.push((name, ty, is_const));
        }
        Ok(bindings)
    }

    fn parameter_input(&mut self, param: &ParamDecl, ty: &Type) -> Result<ExprId, CompilerFailure> {
        let id = self.push_synthetic_expr(
            TypedExprKind::LocalRef {
                ident: param.name.clone(),
                boxed: false,
            },
            ty.clone(),
            param.name.span,
        )?;
        self.typed_ast.parameter_inputs.insert(id);
        Ok(id)
    }

    fn initialize_parameter(
        &mut self,
        param: &ParamDecl,
        storage_ty: &Type,
        value: ExprId,
    ) -> Result<StmtId, CompilerFailure> {
        let statement = self
            .typed_ast
            .try_push_stmt(TypedStmt {
                kind: TypedStmtKind::AssignLocal {
                    ident: param.name.clone(),
                    target_ty: storage_ty.clone(),
                    value,
                    boxed: false,
                    narrowed_shadow_ty: None,
                },
                span: param.name.span,
            })
            .map_err(arena_failure)?;
        self.typed_ast.parameter_initializations.insert(statement);
        Ok(statement)
    }

    pub(super) fn record_authored_parameter_type(&mut self, span: Span, ty: &Type) {
        self.typed_ast.authored_parameter_types.insert(
            (span.file.0, span.start, span.end),
            super::generic::erase_generic_params(ty),
        );
    }

    fn check_parameter_default(
        &mut self,
        param: &ParamDecl,
        declared: &Type,
        default_ty: &Type,
        span: Span,
    ) {
        if super::assignable::assignable(default_ty, declared, self.resolver()) {
            return;
        }
        let Some(concrete) = super::signatures::without_type_vars(declared) else {
            self.error_with_help(span, format!(
                "parameter `{}` cannot have a default value: its type `{declared}` is chosen by the caller", param.name.name,
            ), vec!["use a value of the same type parameter, or a type independent of the caller's choice".into()]);
            return;
        };
        let help = if concrete == *declared {
            Vec::new()
        } else {
            vec![format!(
                "the caller picks what a type parameter stands for, so the default has to fit `{concrete}` — the rest of `{declared}`"
            )]
        };
        self.error_with_help(span, format!("default value of type `{default_ty}` is not assignable to parameter type `{declared}`"), help);
    }

    fn infer_parameter_destructuring(
        &mut self,
        param: &ParamDecl,
        prologue: &mut Vec<StmtId>,
    ) -> Result<(), CompilerFailure> {
        let statements = self
            .ast
            .parameter_bindings
            .get(&param.name.span)
            .cloned()
            .unwrap_or_default();
        for statement in statements {
            if let Some(statement) = self.infer_parameter_binding_stmt(statement)? {
                self.typed_ast.parameter_initializations.insert(statement);
                prologue.push(statement);
            }
        }
        Ok(())
    }

    pub(super) fn prepend_parameter_defaults(
        &mut self,
        mut prologue: Vec<StmtId>,
        body: StmtId,
    ) -> Result<StmtId, CompilerFailure> {
        if prologue.is_empty() {
            return Ok(body);
        }
        let span = self.typed_ast.try_stmt(body).map_err(arena_failure)?.span;
        let count = prologue.len();
        prologue.push(body);
        let wrapped = self
            .typed_ast
            .try_push_stmt(TypedStmt {
                kind: TypedStmtKind::Block(prologue),
                span,
            })
            .map_err(arena_failure)?;
        self.typed_ast
            .parameter_default_prologues
            .insert(wrapped, count);
        Ok(wrapped)
    }

    pub(super) fn local_storage_read(
        &mut self,
        ident: crate::Ident,
        storage_ty: &Type,
        read_ty: &Type,
    ) -> Result<TypedExprKind, CompilerFailure> {
        let kind = TypedExprKind::LocalRef {
            ident: ident.clone(),
            boxed: false,
        };
        if storage_ty == read_ty {
            return Ok(kind);
        }
        let value = self.push_synthetic_expr(kind, storage_ty.clone(), ident.span)?;
        Ok(TypedExprKind::Cast {
            value,
            target_ty: read_ty.clone(),
            check: None,
        })
    }
}

/// The type a parameter has once its default ran: `strip_undefined`, except
/// that a parameter of type `undefined` alone has no value left, which is
/// `never` here rather than the stripper's poisoned `Error`.
pub(super) fn without_undefined(ty: &Type) -> Type {
    if matches!(ty.peel(), Type::Undefined) {
        return Type::Never;
    }
    super::narrowing::strip_undefined(ty)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialized_parameter_reads_distinguish_source_and_omission() {
        for default in ["definiteValue()", "optionalValue()"] {
            let result = if default == "definiteValue()" {
                "number"
            } else {
                "number | undefined"
            };
            let expected = if default == "definiteValue()" {
                Type::Number
            } else {
                Type::union(vec![Type::Number, Type::Undefined])
            };
            for kind in ["inferred", "annotated", "contextual"] {
                for form in ["declaration", "arrow", "expression"] {
                    if kind == "contextual" && form == "declaration" {
                        continue;
                    }
                    let callback = if kind == "contextual" {
                        String::new()
                    } else {
                        format!(": () => {result}")
                    };
                    let annotation = if kind == "annotated" {
                        ": number | undefined"
                    } else {
                        ""
                    };
                    let params = format!(
                        "first{callback} = () => read(), read{callback} = () => value, value{annotation} = {default}"
                    );
                    let context = if kind == "contextual" {
                        format!(
                            ": (first?: () => {result}, read?: () => {result}, value?: number) => {result}"
                        )
                    } else {
                        String::new()
                    };
                    let function = match form {
                        "declaration" => {
                            format!("function example({params}): {result} {{ return first(); }}")
                        }
                        "arrow" => format!("const example{context} = ({params}) => first();"),
                        _ => format!(
                            "const example{context} = function({params}) {{ return first(); }};"
                        ),
                    };
                    let source = format!(
                        "function definiteValue(): number {{ return 7; }} function optionalValue(): number | undefined {{ return undefined; }} {function}"
                    );
                    let (typed, diagnostics) = super::super::test_support::run(&source);
                    assert!(
                        diagnostics.is_empty(),
                        "{kind}/{form}/{default}: {diagnostics:?}"
                    );
                    let reads = typed
                        .expr_ids()
                        .unwrap()
                        .filter_map(|id| match &typed.try_expr(id).unwrap().kind {
                            TypedExprKind::Closure {
                                params,
                                return_type,
                                ..
                            } if params.is_empty() => Some(return_type),
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    assert!(!reads.is_empty());
                    assert!(
                        reads.iter().all(|ty| **ty == expected),
                        "{kind}/{form}/{default}: {reads:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn default_parameter_write_contract_survives_initial_read_refinement() {
        let source =
            include_str!("../../../tests/fixtures/undefined_parameter_default_source_types.ts");
        let (_, diagnostics) = super::super::test_support::run(source);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn nested_parameter_type_trials_have_bounded_growth() {
        fn expression_count(depth: usize) -> usize {
            let mut initializer = String::from("7");
            for _ in 0..depth {
                initializer = format!(
                    "(function(value: number | undefined = {initializer}): number {{ return value; }})()"
                );
            }
            let source = format!(
                "const example: (read?: () => number, value?: number) => number = (read = () => value, value = {initializer}) => read();"
            );
            let (typed, diagnostics) = super::super::test_support::run(&source);
            assert!(diagnostics.is_empty(), "{diagnostics:?}");
            typed.exprs_len()
        }
        // Compilation runs on the documented compiler stack; the test
        // harness's default thread is far smaller.
        crate::type_size::tests::on_compiler_stack(|| {
            let shallow = expression_count(4);
            let deep = expression_count(8);
            assert!(
                deep < shallow * 4,
                "initializer trial growth: {shallow} -> {deep}"
            );
        });
    }

    #[test]
    fn earlier_default_closure_retains_later_inferred_undefined() {
        for later_default in ["optionalValue()", "7"] {
            let source = format!(
                "function optionalValue(): number | undefined {{ return undefined; }}
                 function readLater(read: () => number | undefined = () => value,
                                    value = {later_default}): number | undefined {{
                   return read();
                 }}"
            );
            let (typed, diagnostics) = super::super::test_support::run(&source);
            assert!(diagnostics.is_empty(), "{diagnostics:?}");
            let return_type = typed
                .expr_ids()
                .unwrap()
                .find_map(|id| match &typed.try_expr(id).unwrap().kind {
                    TypedExprKind::Closure { return_type, .. } => Some(return_type),
                    _ => None,
                })
                .expect("earlier default closure");
            let expected = if later_default == "7" {
                Type::Number
            } else {
                Type::union(vec![Type::Number, Type::Undefined])
            };
            assert_eq!(*return_type, expected);
        }
    }

    #[test]
    fn mutable_pattern_declaration_slots_keep_literal_unions() {
        let (typed, diagnostics) = super::super::test_support::run_with_lowered_patterns(
            r#"
            function objectKind({ kind }: { kind: "A" | "B" }): "A" | "B" {
                const initial: "A" | "B" = kind;
                kind = "B";
                return initial;
            }
            function tupleValue([value = 1]: [(1 | 3)?] = []): 1 | 3 {
                const initial: 1 | 3 = value;
                value = 3;
                return initial;
            }
            "#,
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let bindings: std::collections::BTreeMap<_, _> = typed
            .parameter_initializations
            .iter()
            .filter_map(|id| match &typed.try_stmt(*id).unwrap().kind {
                TypedStmtKind::Let { name, ty, .. } => Some((name.name.as_str(), ty.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(
            bindings.get("kind"),
            Some(&Type::union(vec![
                Type::StringLiteral("A".into()),
                Type::StringLiteral("B".into()),
            ])),
        );
        assert_eq!(
            bindings.get("value"),
            Some(&Type::union(vec![
                Type::NumberLiteral(crate::types::LiteralF64(1.0)),
                Type::NumberLiteral(crate::types::LiteralF64(3.0)),
            ])),
        );
    }
}
