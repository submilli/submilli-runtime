//! Injected arena corruption is an internal failure, not a language diagnostic.
use std::collections::BTreeMap;

use submilli_engine::{
    Ast, ExprId, FileId, ModulePath, Sources, StmtId, StmtKind,
    compiler_error::{CompileError, CompilerFailure, CompilerStage},
};

fn parse(source: &str, file: FileId) -> Ast {
    let mut lexer = submilli_engine::Asi::new(source, file);
    let mut tokens = Vec::new();
    loop {
        let token = lexer.next_token();
        let eof = matches!(token.kind, submilli_engine::TokenKind::Eof);
        tokens.push(token);
        if eof {
            break;
        }
    }
    submilli_engine::parser::parse_checked(source, tokens, file)
        .unwrap()
        .0
}

fn infer(
    source: &str,
    ast: &Ast,
) -> Result<(submilli_engine::TypedAst, Vec<submilli_engine::Diagnostic>), CompileError> {
    let (prelude, host, _) =
        submilli_engine::runtime::prelude::cached_runtime_package_declarations();
    let packages: Vec<_> = prelude.iter().chain(host).collect();
    submilli_engine::typechecker::infer::infer_with_transitive_checked(
        source,
        "main",
        ast,
        &packages,
        &[],
    )
}

fn assert_internal(error: &CompileError) {
    assert!(
        matches!(
            &error.fatal,
            Some(CompilerFailure::Internal {
                stage: CompilerStage::Infer,
                span: None,
                ..
            })
        ),
        "{error:?}"
    );
    assert!(error.to_string().contains("invalid node ID"), "{error}");
}

#[test]
fn inference_rejects_invalid_statement_and_expression_ids() {
    let source = "function main(): number { return 1; }";
    let original = submilli_engine::lower_patterns(parse(source, FileId(0))).unwrap();
    for corrupt_expression in [false, true] {
        let mut ast = original.clone();
        if corrupt_expression {
            for id in ast.stmt_ids().unwrap() {
                if let StmtKind::Return(value) = &mut ast.try_stmt_mut(id).unwrap().kind {
                    *value = Some(ExprId(u32::MAX));
                }
            }
        } else {
            ast.top_level.push(StmtId(u32::MAX));
        }
        assert_internal(&infer(source, &ast).unwrap_err());
    }
    let (_, diagnostics) = infer(source, &original).unwrap();
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn binding_failure_retains_earlier_language_diagnostics() {
    let source = "function main(): number { let value = value; return 1; }";
    let mut ast = submilli_engine::lower_patterns(parse(source, FileId(0))).unwrap();
    ast.top_level.push(StmtId(u32::MAX));
    let error = infer(source, &ast).unwrap_err();
    assert_internal(&error);
    assert!(
        error
            .diagnostics
            .iter()
            .any(|d| d.message.contains("before")),
        "{:?}",
        error.diagnostics
    );
}

#[test]
fn lowering_rejects_bad_top_level_and_pattern_body_ids() {
    let mut ast = Ast::new();
    ast.top_level.push(StmtId(u32::MAX));
    let error = submilli_engine::lower_patterns(ast).unwrap_err();
    assert_internal(&error.into());

    let source = "function main([x]: number[]): number { return x; }";
    let mut ast = parse(source, FileId(0));
    let root = ast.top_level[0];
    let StmtKind::Function { body, .. } = &mut ast.try_stmt_mut(root).unwrap().kind else {
        panic!("function")
    };
    *body = StmtId(u32::MAX);
    assert_internal(&submilli_engine::lower_patterns(ast).unwrap_err().into());
    submilli_engine::lower_patterns(parse(source, FileId(0))).unwrap();
}

#[test]
fn export_metadata_reads_are_checked_after_binding_analysis() {
    let source = "export function value(): number { return 1; }";
    let mut ast = submilli_engine::lower_patterns(parse(source, FileId(0))).unwrap();
    ast.exported_decls[0].stmt = StmtId(u32::MAX);
    assert_internal(&infer(source, &ast).unwrap_err());
}

#[test]
fn package_graph_failure_preserves_earlier_import_diagnostic() {
    let mut sources = Sources::new();
    let source =
        "import { value } from './missing'; export function read(): number { return value(); }";
    let file = sources.add("lib.ts", source).unwrap();
    let mut ast = submilli_engine::lower_patterns(parse(source, file)).unwrap();
    ast.top_level.push(StmtId(u32::MAX));
    let error = submilli_engine::typechecker::infer::infer_package_checked(
        "test",
        ModulePath::from("lib"),
        vec![(ModulePath::from("lib"), file, &ast)],
        &sources,
        BTreeMap::new(),
        BTreeMap::new(),
    )
    .unwrap_err();
    assert_internal(&error);
    assert!(!error.diagnostics.is_empty());
    assert!(
        error
            .diagnostics
            .iter()
            .any(|d| d.message.contains("missing"))
    );
}

#[test]
fn later_module_arena_failure_preserves_earlier_module_diagnostics() {
    let mut sources = Sources::new();
    let first = "export const bad: number = 'wrong';";
    let last = "export function read(): number { return 1; }";
    let first_file = sources.add("a.ts", first).unwrap();
    let last_file = sources.add("lib.ts", last).unwrap();
    let first_ast = submilli_engine::lower_patterns(parse(first, first_file)).unwrap();
    let mut last_ast = submilli_engine::lower_patterns(parse(last, last_file)).unwrap();
    let root = last_ast.top_level[0];
    let StmtKind::Function { body, .. } = &mut last_ast.try_stmt_mut(root).unwrap().kind else {
        panic!("function")
    };
    *body = StmtId(u32::MAX);
    let (prelude, host, _) =
        submilli_engine::runtime::prelude::cached_runtime_package_declarations();
    let packages = prelude
        .iter()
        .chain(host)
        .cloned()
        .map(|p| (p.package_name.clone(), p))
        .collect();
    let error = submilli_engine::typechecker::infer::infer_package_checked(
        "test",
        ModulePath::from("lib"),
        vec![
            (ModulePath::from("a"), first_file, &first_ast),
            (ModulePath::from("lib"), last_file, &last_ast),
        ],
        &sources,
        packages,
        BTreeMap::new(),
    )
    .unwrap_err();
    assert_internal(&error);
    assert!(
        error.diagnostics.iter().any(|d| d.span.file == first_file),
        "{:?}",
        error.diagnostics
    );
}

#[test]
fn inference_rejects_wrong_node_kinds_without_losing_prior_diagnostics() {
    use submilli_engine::ExprKind;
    let source = "function main(): number { let early = early; let x = 0; return (x = 1); }";
    let mut ast = submilli_engine::lower_patterns(parse(source, FileId(0))).unwrap();
    let target = ast
        .expr_ids()
        .unwrap()
        .find_map(|id| match ast.try_expr(id).unwrap().kind {
            ExprKind::Assign { target, .. } => Some(target),
            _ => None,
        })
        .unwrap();
    ast.try_expr_mut(target).unwrap().kind = ExprKind::Number(0.0);
    let error = infer(source, &ast).unwrap_err();
    assert!(matches!(
        error.fatal,
        Some(CompilerFailure::Internal { span: None, .. })
    ));
    assert!(error.to_string().contains("valid targets"));
    assert!(
        error
            .diagnostics
            .iter()
            .any(|d| d.message.contains("before"))
    );
}

#[test]
fn inference_rejects_wrong_function_and_loop_body_kinds() {
    use submilli_engine::ExprKind;
    for source in [
        "function main(): number { const f = function(): number { return 1; }; return f(); }",
        "function main(): number { while (false) { return 1; } return 0; }",
        "function main(): number { return 1; }",
    ] {
        let mut ast = submilli_engine::lower_patterns(parse(source, FileId(0))).unwrap();
        if source.contains("const f") {
            let function = ast
                .expr_ids()
                .unwrap()
                .find_map(|id| match ast.try_expr(id).unwrap().kind {
                    ExprKind::FunctionExpression { function, .. } => Some(function),
                    _ => None,
                })
                .unwrap();
            ast.try_expr_mut(function).unwrap().kind = ExprKind::Number(0.0);
        } else if source.contains("while") {
            let return_id = ast
                .stmt_ids()
                .unwrap()
                .find(|&id| matches!(ast.try_stmt(id).unwrap().kind, StmtKind::Return(_)))
                .unwrap();
            for id in ast.stmt_ids().unwrap() {
                if let StmtKind::While { body, .. } = &mut ast.try_stmt_mut(id).unwrap().kind {
                    *body = return_id;
                }
            }
        } else {
            let root = ast.top_level[0];
            if let StmtKind::Function { return_type, .. } =
                &mut ast.try_stmt_mut(root).unwrap().kind
            {
                *return_type = None;
            }
        }
        let error = infer(source, &ast).unwrap_err();
        assert!(
            matches!(
                error.fatal,
                Some(CompilerFailure::Internal { span: None, .. })
            ),
            "{error:?}"
        );
    }
}

#[test]
fn inference_fits_production_worker_stack() {
    use std::process::Command;
    use std::time::{Duration, Instant};

    if std::env::var_os("SUB633_INFERENCE_STACK_CHILD").is_some() {
        for stack_size in [2 * 1024 * 1024, 8 * 1024 * 1024] {
            std::thread::Builder::new()
                .stack_size(stack_size)
                .spawn(|| {
                    let nested_finally =
                        include_str!("fixtures/exceptions/deep_finally_transfers.ts");
                    let binary_chain = format!(
                        "function main(): number {{ return {}; }}",
                        vec!["1"; 32].join(" + ")
                    );
                    let logical_chain = format!(
                        "function main(): boolean {{ return {}; }}",
                        vec!["true"; 32].join(" && ")
                    );
                    let equality_chain = format!(
                        "function main(): boolean {{ return {}; }}",
                        vec!["true"; 32].join(" === ")
                    );
                    for source in [
                        nested_finally,
                        &binary_chain,
                        &logical_chain,
                        &equality_chain,
                        "function main(): number { return 42; }",
                    ] {
                        let compiled = submilli_engine::compile::compile_script_checked(
                            source,
                            "stack.ts",
                            FileId(0),
                            &[],
                            &[],
                        )
                        .unwrap();
                        assert!(!compiled.wasm.is_empty());
                    }
                })
                .unwrap()
                .join()
                .unwrap();
        }
        return;
    }
    // Keep any stack regression inside a bounded child rather than aborting the
    // test runner. Explicit worker sizes apply equally to debug and release.
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "inference_fits_production_worker_stack",
            "--nocapture",
        ])
        .env("SUB633_INFERENCE_STACK_CHILD", "1")
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "inference child failed: {status}");
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("inference child timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
