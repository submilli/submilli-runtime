//! Exercise the expression-scoped narrowings and plain conditions ts-rust emits.
use crate::{TypedAst, TypedExprKind, TypedStmtKind};

#[test]
fn enum_member_union_truthiness_from_an_alternate_front_end() {
    let source = include_str!("../../tests/fixtures/enum_member_union_truthiness.ts");
    let mut tree = super::tests::type_check(source);
    // ts-rust leaves the constant ternary branches plain. Our inferer's
    // branch scopes otherwise cause runtime-values lowering to erase the union.
    for id in tree.expr_ids().unwrap() {
        if let TypedExprKind::Narrowed { inner, .. } = tree.try_expr(id).unwrap().kind {
            *tree.try_expr_mut(id).unwrap() = tree.try_expr(inner).unwrap().clone();
        }
    }
    assert_eq!(run_tree(source, &tree), "TF|TF");
}

#[test]
fn enum_narrowings_from_an_alternate_front_end() {
    for (parameter, argument, guard, expected) in [
        ("number", "2", "n === E.B", "F2"),
        ("number | undefined", "E.B", "n === E.B", "F2"),
        ("unknown", "E.B", "n === E.B", "F2"),
        ("string", "S.B", "n === S.B", "Fb"),
        ("number", "2", "n === 1 || n === 2", "F2"),
    ] {
        let source = format!(
            r#"
            enum E {{ A = 1, B }}
            enum S {{ A = "", B = "b" }}
            function f(n: {parameter}): string {{
                if ({guard}) {{ return String(n); }}
                return "F";
            }}
            function main(): string {{ return f({}).concat(f({argument})); }}
        "#,
            if parameter == "string" { "\"x\"" } else { "0" }
        );
        let mut tree = super::tests::type_check(&source);
        assert!(expression_scoped_narrowings(&mut tree) > 0, "{parameter}");
        assert_eq!(run_tree(&source, &tree), expected, "{parameter}");
    }
}

/// Move each statement's narrowing onto its reads, as an alternate front end
/// may do. Retain LocalNarrowRef: replacing it with LocalRef would read the
/// original slot and fail to exercise the narrowed shadow's representation.
fn expression_scoped_narrowings(tree: &mut TypedAst) -> usize {
    let mut reads = 0;
    for stmt in tree.stmt_ids().unwrap() {
        let TypedStmtKind::NarrowRegion {
            path,
            source,
            binding,
            cast_info,
            body,
        } = tree.try_stmt(stmt).unwrap().kind.clone()
        else {
            continue;
        };
        tree.try_stmt_mut(stmt).unwrap().kind = TypedStmtKind::Block(vec![body]);
        for id in tree.expr_ids().unwrap() {
            if !matches!(&tree.try_expr(id).unwrap().kind,
                TypedExprKind::LocalNarrowRef { binding: read_binding, .. } if read_binding == &binding)
            {
                continue;
            }
            reads += 1;
            let inner = tree
                .try_push_expr(tree.try_expr(id).unwrap().clone())
                .unwrap();
            tree.try_expr_mut(id).unwrap().kind = TypedExprKind::Narrowed {
                path: path.clone(),
                source,
                binding: binding.clone(),
                cast_info: cast_info.clone(),
                inner,
            };
        }
    }
    reads
}

fn run_tree(source: &str, tree: &TypedAst) -> String {
    let (prelude, host, internal) = crate::runtime::prelude::cached_runtime_package_declarations();
    let dependencies: Vec<_> = prelude.iter().chain(host).chain(internal).collect();
    let generated =
        super::codegen_with_type_info(source, "enum.ts", crate::FileId(0), tree, &dependencies)
            .expect("code generation");
    wasmparser::Validator::new()
        .validate_all(&generated.wasm)
        .expect("valid Wasm");
    pollster::block_on(crate::RuntimeConfig::default().run(&generated.wasm))
        .expect("execution")
        .value
        .expect("main result")
}
