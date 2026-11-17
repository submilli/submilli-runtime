//! Alternate front ends supply these valid TypeScript forms directly. The engine
//! front end rejects several of them, so marker calls let it build the surrounding
//! tree; each test then replaces only that marker with the reported TypedAst.
use submilli_engine::{
    FileId, Ident, RuntimeConfig, Type, TypedAst, TypedExpr, TypedExprKind, TypedObjectMember,
    TypedStmtKind, UnOp,
};

fn run(source: &str, prepare: impl FnOnce(&mut TypedAst) + Send + 'static) -> String {
    let source = source.to_owned();
    std::thread::Builder::new()
        .stack_size(submilli_engine::compiler_limits::COMPILER_STACK_BYTES)
        .spawn(move || {
            let (mut ast, diagnostics) =
                submilli_engine::compile::typecheck_to_typed_ast(&source, FileId(0));
            assert!(diagnostics.is_empty(), "{diagnostics:?}");
            prepare(&mut ast);
            let ast = submilli_engine::desugar(submilli_engine::capture(ast).unwrap(), FileId(0))
                .unwrap();
            let (prelude, host, internal) =
                submilli_engine::runtime::prelude::cached_runtime_package_declarations();
            let dependencies = prelude
                .iter()
                .chain(host)
                .chain(internal)
                .collect::<Vec<_>>();
            let wasm = submilli_engine::codegen::codegen(
                &source,
                "typed-values.ts",
                FileId(0),
                &ast,
                &dependencies,
            )
            .unwrap();
            pollster::block_on(RuntimeConfig::default().run(&wasm))
                .expect("valid typed program loads and runs")
                .value
                .expect("main returns a value")
        })
        .unwrap()
        .join()
        .unwrap()
}

#[test]
fn generic_spread_copies_own_fields() {
    let source = include_str!("typed_value_sources/spread.ts").replace("{ ...t }", "copy(t)");
    let source = format!("function copy<T>(t: T): T {{ return t; }}\n{source}");
    assert_eq!(
        run(&source, |ast| {
            ast.shapes
                .push(submilli_engine::Shape::from_type(&Type::empty_object()).unwrap());
            let mut count = 0;
            for id in ast.expr_ids().unwrap() {
                let expr = ast.try_expr(id).unwrap().clone();
                let TypedExprKind::GenericCall { mangled, args, .. } = &expr.kind else {
                    continue;
                };
                if mangled != &submilli_engine::mangle::package_symbol("main", "copy") {
                    continue;
                }
                let object = ast
                    .try_push_expr(TypedExpr {
                        span: expr.span,
                        ty: Type::empty_object(),
                        kind: TypedExprKind::ObjectLiteral {
                            fields: vec![],
                            members: vec![TypedObjectMember::Spread {
                                source: args[0].expr,
                                by_name: false,
                            }],
                        },
                    })
                    .unwrap();
                ast.try_expr_mut(id).unwrap().kind = TypedExprKind::Cast {
                    value: object,
                    target_ty: expr.ty,
                    check: None,
                };
                count += 1;
            }
            assert_eq!(count, 4);
        }),
        "0"
    );
}

#[test]
fn function_spread_has_no_enumerable_fields() {
    // A placeholder structural view lets the engine infer the ordinary object
    // spread layout. Remove that view to supply the function-typed source.
    let source = include_str!("typed_value_sources/function-spread.ts")
        .replace("...f,", "...(f as object),")
        .replace("...five", "...(five as object)")
        .replace("...getFunction()", "...(getFunction() as object)");
    assert_eq!(
        run(&source, |ast| {
            let mut count = 0;
            for id in ast.expr_ids().unwrap() {
                let expr = ast.try_expr(id).unwrap().clone();
                if let TypedExprKind::Cast { value, .. } = expr.kind
                    && matches!(
                        ast.try_expr(value).unwrap().ty.peel(),
                        Type::Function { .. }
                    )
                {
                    *ast.try_expr_mut(id).unwrap() = ast.try_expr(value).unwrap().clone();
                    count += 1;
                }
            }
            assert_eq!(count, 3);
        }),
        "0"
    );
}

#[test]
fn primitive_property_write_throws_after_evaluating_value() {
    let source = include_str!("typed_value_sources/write.ts")
        .replace("s.toString = value()", "write(s, value())");
    let source = format!("function write(s: unknown, f: () => string): void {{}}\n{source}");
    assert_eq!(
        run(&source, |ast| {
            let mut count = 0;
            for id in ast.stmt_ids().unwrap() {
                let stmt = ast.try_stmt(id).unwrap().clone();
                let TypedStmtKind::Expr(expr) = stmt.kind else {
                    continue;
                };
                let TypedExprKind::Call { mangled, args, .. } =
                    ast.try_expr(expr).unwrap().kind.clone()
                else {
                    continue;
                };
                if mangled != submilli_engine::mangle::package_symbol("main", "write") {
                    continue;
                }
                let receiver_ty = ast.try_expr(args[0]).unwrap().ty.clone();
                // Declarations can retain the alias reference without its body.
                if let Type::Alias {
                    mangled,
                    package,
                    name,
                    args: type_args,
                    ..
                } = receiver_ty
                {
                    ast.try_expr_mut(args[0]).unwrap().ty = Type::AliasRef {
                        mangled,
                        package,
                        name,
                        args: type_args,
                    };
                } else if receiver_ty == Type::String {
                    // Exercise the primitive intersection used by branded strings too.
                    ast.try_expr_mut(args[0]).unwrap().ty = Type::Intersection(vec![
                        Type::String,
                        Type::Object {
                            fields: std::collections::BTreeMap::from([(
                                "__brand".into(),
                                submilli_engine::ObjectField::required(Type::StringLiteral(
                                    "id".into(),
                                )),
                            )]),
                            index: None,
                            more_indexes: vec![],
                        },
                    ]);
                }
                if ast.try_expr(args[0]).unwrap().ty.peel() == &Type::Boolean {
                    ast.try_expr_mut(args[0]).unwrap().ty = Type::Union(vec![
                        Type::Intersection(vec![
                            Type::TypeVar("T".into()),
                            Type::BooleanLiteral(false),
                        ]),
                        Type::Intersection(vec![
                            Type::TypeVar("T".into()),
                            Type::BooleanLiteral(true),
                        ]),
                    ]);
                }
                ast.try_stmt_mut(id).unwrap().kind = TypedStmtKind::AssignField {
                    receiver: args[0],
                    value: args[1],
                    name: Ident {
                        name: "toString".into(),
                        span: stmt.span,
                    },
                };
                count += 1;
            }
            assert_eq!(count, 8);
        }),
        "0"
    );
}

#[test]
fn unary_operators_convert_non_numeric_values() {
    let source = include_str!("typed_value_sources/unary.ts")
        .replace("+id(", "positive(")
        .replace("-id(", "negative(")
        .replace("-u", "negative(u)")
        .replace("-n", "negativeNumeric(n)");
    let source = format!(
        "function positive(x: unknown): number {{ return 0; }}\nfunction negative(x: unknown): number {{ return 0; }}\nfunction negativeNumeric(x: number | bigint): number | bigint {{ return 0; }}\n{source}"
    );
    assert_eq!(
        run(&source, |ast| {
            let mut count = 0;
            for id in ast.expr_ids().unwrap() {
                let expr = ast.try_expr(id).unwrap().clone();
                let TypedExprKind::Call { mangled, args, .. } = expr.kind else {
                    continue;
                };
                let op = if mangled == submilli_engine::mangle::package_symbol("main", "positive") {
                    UnOp::Pos
                } else if mangled == submilli_engine::mangle::package_symbol("main", "negative")
                    || mangled == submilli_engine::mangle::package_symbol("main", "negativeNumeric")
                {
                    UnOp::Neg
                } else {
                    continue;
                };
                ast.try_expr_mut(id).unwrap().kind = TypedExprKind::Unary {
                    op,
                    operand: args[0],
                };
                count += 1;
            }
            assert_eq!(count, 11);
        }),
        "0"
    );
}
