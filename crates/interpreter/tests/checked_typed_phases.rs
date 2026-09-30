//! Corrupt IDs below are injected internal state, not guest-input reproducers.
use interpreter::{
    ExprId, FileId, StmtId, TypedAst, TypedExprKind, TypedStmtKind,
    compiler_error::{CompilerFailure, CompilerStage},
};

const SOURCE: &str = "function main(): number { return 1; }";

fn typed(source: &str) -> TypedAst {
    let (ast, diagnostics) = interpreter::compile::typecheck_to_typed_ast(source, FileId(0));
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    ast
}

fn assert_invalid_id(failure: CompilerFailure) {
    assert!(
        matches!(failure, CompilerFailure::Internal { span: None, .. }),
        "{failure}"
    );
    assert!(failure.to_string().contains("invalid node ID"), "{failure}");
}

fn corrupt_return(ast: &mut TypedAst) {
    let mut replaced = false;
    for id in ast.stmt_ids().unwrap() {
        if let TypedStmtKind::Return(value) = &mut ast.try_stmt_mut(id).unwrap().kind {
            *value = Some(ExprId(u32::MAX));
            replaced = true;
        }
    }
    assert!(replaced);
}

#[test]
fn rules_and_capture_propagate_invalid_expression_and_statement_ids() {
    let original = typed(SOURCE);
    for corrupt_expression in [false, true] {
        let mut ast = original.clone();
        if corrupt_expression {
            corrupt_return(&mut ast);
        } else {
            ast.functions[0].body = StmtId(u32::MAX);
        }
        assert_invalid_id(interpreter::check(&ast).unwrap_err().fatal.unwrap());
        assert_invalid_id(interpreter::capture(ast).unwrap_err());
    }
    assert!(interpreter::check(&original).unwrap().is_empty());
    interpreter::capture(original).unwrap();
}

#[test]
fn rules_keep_diagnostics_collected_before_a_fatal_failure() {
    let source = "function first(): number { return 1; } function main(): number { return 2; }";
    let mut ast = typed(source);
    let first = ast.functions[0].body;
    ast.try_stmt_mut(first).unwrap().kind = TypedStmtKind::Block(Vec::new());
    ast.functions[1].body = StmtId(u32::MAX);
    let error = interpreter::check(&ast).unwrap_err();
    assert_invalid_id(error.fatal.unwrap());
    assert!(
        error
            .diagnostics
            .iter()
            .any(|d| d.message.contains("does not return")),
        "{:?}",
        error.diagnostics
    );
}

#[test]
fn desugaring_rejects_an_invalid_loop_body() {
    let source = "function main(): void { do {} while (false); }";
    let original = typed(source);
    let mut ast = original.clone();
    let mut replaced = false;
    for id in ast.stmt_ids().unwrap() {
        if let TypedStmtKind::DoWhile { body, .. } = &mut ast.try_stmt_mut(id).unwrap().kind {
            *body = StmtId(u32::MAX);
            replaced = true;
        }
    }
    assert!(replaced);
    assert_invalid_id(interpreter::desugar(ast, FileId(0)).unwrap_err());
    interpreter::desugar(original, FileId(0)).unwrap();
}

#[test]
fn indirect_arena_reads_validate_ids_even_when_metadata_exists() {
    let mut ast = typed(SOURCE);
    let id = ExprId(u32::MAX);
    ast.runtime_source_types
        .insert(id, interpreter::Type::Number);
    assert!(ast.source_type(id).is_err());
    assert!(ast.is_effect_free(id).is_err());
    let literal = ast
        .expr_ids()
        .unwrap()
        .find(|id| matches!(ast.try_expr(*id).unwrap().kind, TypedExprKind::Number(_)))
        .unwrap();
    assert!(ast.is_effect_free(literal).unwrap());
    assert_eq!(
        ast.source_type(literal).unwrap(),
        &interpreter::Type::Number
    );
}

#[test]
fn codegen_returns_failure_instead_of_partial_wasm() {
    let original =
        interpreter::desugar(interpreter::capture(typed(SOURCE)).unwrap(), FileId(0)).unwrap();
    let (prelude, host, internal) =
        interpreter::runtime::prelude::cached_runtime_package_declarations();
    let dependencies: Vec<_> = prelude.iter().chain(host).chain(internal).collect();
    for corrupt_expression in [false, true] {
        let mut ast = original.clone();
        if corrupt_expression {
            corrupt_return(&mut ast);
        } else {
            ast.functions[0].body = StmtId(u32::MAX);
        }
        let failure =
            interpreter::codegen::codegen(SOURCE, "typed.ts", FileId(0), &ast, &dependencies)
                .unwrap_err();
        assert!(matches!(
            failure,
            CompilerFailure::Internal {
                stage: CompilerStage::Codegen,
                ..
            }
        ));
        assert_invalid_id(failure);
    }
    assert!(
        !interpreter::codegen::codegen(SOURCE, "typed.ts", FileId(0), &original, &dependencies)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn invalid_postfix_kind_is_a_typed_failure() {
    let source =
        "function main(): number { const obj = { value: 1 }; obj.value++; return obj.value; }";
    let mut ast = typed(source);
    let mut replaced = false;
    for id in ast.expr_ids().unwrap() {
        if let TypedExprKind::PostfixUnary { op, .. } = &mut ast.try_expr_mut(id).unwrap().kind {
            *op = interpreter::PostfixOp::NonNullAssert;
            replaced = true;
        }
    }
    assert!(replaced);
    assert!(matches!(
        interpreter::desugar(ast, FileId(0)).unwrap_err(),
        CompilerFailure::Internal {
            stage: CompilerStage::Infer,
            ..
        }
    ));
}

#[test]
fn capability_failure_retains_prior_warnings_and_does_not_return_a_filter() {
    use interpreter::{
        DocCapability, DocCapabilityBinding, DocCapabilityBindingKind, Param, Span, Type,
    };
    let span = Span::at(FileId(0));
    let binding = |param: &str| DocCapabilityBinding {
        field: "value".into(),
        field_span: span,
        kind: DocCapabilityBindingKind::Parameter {
            param: param.into(),
            path: Vec::new(),
            span,
        },
    };
    let tag = DocCapability {
        tag_span: span,
        capability: "test.read".into(),
        capability_span: span,
        bindings: vec![binding("missing"), binding("value")],
        description: String::new(),
        diagnostics: Vec::new(),
    };
    let params = [Param::new("value", Type::Number)];
    let ast = typed(SOURCE);
    let error = interpreter::derive_call_site_capability(&tag, &params, &ast, &[ExprId(u32::MAX)])
        .unwrap_err();
    assert_invalid_id(error.fatal.unwrap());
    assert_eq!(error.diagnostics.len(), 1);
    assert!(error.diagnostics[0].message.contains("unknown parameter"));
    let value = ast
        .expr_ids()
        .unwrap()
        .find(|id| matches!(ast.try_expr(*id).unwrap().kind, TypedExprKind::Number(_)))
        .unwrap();
    assert!(
        interpreter::derive_call_site_capability(&tag, &params, &ast, &[value])
            .unwrap()
            .filter
            .is_some()
    );
}

#[test]
fn desugaring_validates_rewritten_loop_references() {
    for source in [
        "function main(): void { for (;;) {} }",
        "function main(): void { for (const x of [1]) {} }",
        "function main(): void { do {} while (false); }",
    ] {
        let original = typed(source);
        let cases = if source.contains("for (;;)") { 4 } else { 2 };
        for corrupt in 0..cases {
            let mut ast = original.clone();
            let mut replaced = false;
            for id in ast.stmt_ids().unwrap() {
                match &mut ast.try_stmt_mut(id).unwrap().kind {
                    TypedStmtKind::For {
                        init,
                        body,
                        condition,
                        update,
                    } => {
                        match corrupt {
                            0 => *body = StmtId(u32::MAX),
                            1 => *condition = Some(ExprId(u32::MAX)),
                            2 => *update = Some(StmtId(u32::MAX)),
                            _ => *init = Some(StmtId(u32::MAX)),
                        }
                        replaced = true;
                    }
                    TypedStmtKind::ForOf { iter, body, .. } => {
                        if corrupt == 0 {
                            *iter = ExprId(u32::MAX);
                        } else {
                            *body = StmtId(u32::MAX);
                        }
                        replaced = true;
                    }
                    TypedStmtKind::DoWhile { condition, body } => {
                        if corrupt == 0 {
                            *condition = ExprId(u32::MAX);
                        } else {
                            *body = StmtId(u32::MAX);
                        }
                        replaced = true;
                    }
                    _ => {}
                }
            }
            assert!(replaced);
            assert_invalid_id(interpreter::desugar(ast, FileId(0)).unwrap_err());
        }
        interpreter::desugar(original, FileId(0)).unwrap();
    }
}

#[test]
fn desugaring_validates_postfix_operands() {
    use interpreter::PostfixTarget;
    for source in [
        "function main(): void { const obj = { value: 1 }; obj.value++; }",
        "function main(): void { const arr = [1]; arr[0]++; }",
        "function main(): void { const arr = new Uint8Array(1); arr[0]++; }",
    ] {
        let original = typed(source);
        for corrupt_index in [false, true] {
            let mut ast = original.clone();
            let mut replaced = false;
            for id in ast.expr_ids().unwrap() {
                if let TypedExprKind::PostfixUnary { target, .. } =
                    &mut ast.try_expr_mut(id).unwrap().kind
                {
                    match target {
                        PostfixTarget::Field { receiver, .. } => *receiver = ExprId(u32::MAX),
                        PostfixTarget::Index {
                            receiver, index, ..
                        } => {
                            if corrupt_index {
                                *index = ExprId(u32::MAX);
                            } else {
                                *receiver = ExprId(u32::MAX);
                            }
                        }
                        _ => continue,
                    }
                    replaced = true;
                }
            }
            assert!(replaced);
            assert_invalid_id(interpreter::desugar(ast, FileId(0)).unwrap_err());
        }
        interpreter::desugar(original, FileId(0)).unwrap();
    }
}

#[test]
fn capability_failure_retains_deferred_http_warning() {
    use interpreter::{
        DocCapability, DocCapabilityBinding, DocCapabilityBindingKind, Ident, Param, Span, Type,
        TypedExpr,
    };
    let span = Span::at(FileId(0));
    let mut ast = TypedAst::new();
    let dynamic = ast
        .try_push_expr(TypedExpr {
            kind: TypedExprKind::LocalRef {
                ident: Ident {
                    name: "u".into(),
                    span,
                },
                boxed: false,
            },
            ty: Type::String,
            span,
        })
        .unwrap();
    let binding = |field: &str, param: &str, path: Vec<String>| DocCapabilityBinding {
        field: field.into(),
        field_span: span,
        kind: DocCapabilityBindingKind::Parameter {
            param: param.into(),
            path,
            span,
        },
    };
    let tag = DocCapability {
        tag_span: span,
        capability: "http.get".into(),
        capability_span: span,
        bindings: vec![
            binding("host", "url", vec!["host".into()]),
            binding("other", "other", vec![]),
        ],
        description: String::new(),
        diagnostics: vec![],
    };
    let params = [
        Param::new("url", Type::String),
        Param::new("other", Type::String),
    ];
    let error =
        interpreter::derive_call_site_capability(&tag, &params, &ast, &[dynamic, ExprId(u32::MAX)])
            .unwrap_err();
    assert_invalid_id(error.fatal.unwrap());
    assert_eq!(error.diagnostics.len(), 1);
    assert!(
        error.diagnostics[0]
            .message
            .contains("cannot statically resolve the host")
    );
    let healthy =
        interpreter::derive_call_site_capability(&tag, &params, &ast, &[dynamic, dynamic]).unwrap();
    assert_eq!(healthy.warnings.last(), error.diagnostics.first());
}
