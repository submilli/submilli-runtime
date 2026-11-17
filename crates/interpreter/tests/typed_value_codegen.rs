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
            let generated = submilli_engine::codegen::codegen_with_type_info(
                &source,
                "typed-values.ts",
                FileId(0),
                &ast,
                &dependencies,
            )
            .unwrap();
            let compiled = submilli_engine::CompiledScript {
                wasm: generated.wasm,
                type_info: generated.type_info,
                warnings: vec![],
                timings: Default::default(),
            };
            pollster::block_on(RuntimeConfig::default().run_compiled(&compiled))
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

#[test]
fn delete_field_removes_own_properties() {
    let source = include_str!("typed_value_sources/delete.ts")
        .replace("delete o.p", "deleteMarker(o, \"p\")")
        .replace(
            "delete receiver()[\"p\"]",
            "deleteMarker(receiver(), \"p\")",
        );
    assert_eq!(run_with_delete(&source, 2), "0");
}

#[test]
fn delete_field_preserves_prototype_members() {
    let source = include_str!("typed_value_sources/delete-prototype.ts")
        .replace("delete o.p", "deleteMarker(o, \"p\")")
        .replace("delete method.p", "deleteMarker(method, \"p\")");
    let source = source.replace("delete o.next", "deleteMarker(o, \"next\")");
    assert_eq!(run_with_delete(&source, 3), "0");
}

#[test]
fn delete_field_uses_literal_property_names() {
    let mut source = include_str!("typed_value_sources/delete-keys.ts").to_owned();
    for key in ["雪", "", "get p", "1"] {
        source = source.replace(
            &format!("delete keys[\"{key}\"]"),
            &format!("deleteMarker(keys, \"{key}\")"),
        );
    }
    assert_eq!(run_with_delete(&source, 4), "0");
}

#[test]
fn delete_field_preserves_structural_equality_and_hashing() {
    // Submilli's documented structural equality differs from JS identity.
    let source = r#"
        function main(): number {
            const required = { p: 1, keep: 2 };
            deleteMarker(required, "p");
            const absent: {p?: number; keep: number} = { keep: 2 };
            if (required !== absent || absent !== required) return 1;
            const forward = new Map<object, number>();
            forward.set(required, 42);
            const reverse = new Map<object, number>();
            reverse.set(absent, 43);
            if (forward.get(absent) !== 42 || reverse.get(required) !== 43) return 2;
            const set = new Set<object>();
            set.add(required);
            if (!set.has(absent)) return 3;
            const copied = { ...required };
            if ("p" in copied || Object.hasOwn(copied, "p")) return 4;
            return 0;
        }
    "#;
    assert_eq!(run_with_delete(source, 1), "0");
}

#[test]
fn delete_field_handles_optional_views_of_builtins() {
    let source = include_str!("typed_value_sources/delete-builtins.ts")
        .replace("delete o.toString", "deleteMarker(o, \"toString\")")
        .replace("delete o.length", "deleteMarker(o, \"length\")")
        .replace("delete o.lastIndex", "deleteMarker(o, \"lastIndex\")");
    assert_eq!(run_with_delete(&source, 3), "0");
}

#[test]
fn delete_field_removes_configurable_builtin_metadata() {
    let mut source = include_str!("typed_value_sources/delete-metadata.ts").to_owned();
    for name in ["length", "index", "input", "groups"] {
        source = source.replace(
            &format!("delete o.{name}"),
            &format!("deleteMarker(o, \"{name}\")"),
        );
    }
    assert_eq!(run_with_delete(&source, 4), "0");
}

#[test]
fn deleted_match_metadata_is_checked_by_runtime_casts() {
    // Runtime-checked casts are an intentional Submilli extension to TypeScript.
    let source = r#"
        function acceptsExec(value: unknown): boolean {
            try { const checked = value as RegExpExecArray; return checked.length > 0; }
            catch { return false; }
        }
        function main(): number {
            const m = /(?<a>a)/.exec("a");
            if (m === null || !acceptsExec(m)) return 1;
            deleteMarker(m, "index");
            if (acceptsExec(m)) return 2;
            const optional = m as unknown as RegExpMatchArray;
            if (optional.index !== undefined) return 3;
            const n = /(?<a>a)/.exec("a");
            if (n === null) return 4;
            deleteMarker(n, "input");
            if (acceptsExec(n)) return 5;
            const groups = n.groups as unknown as { [key: string]: unknown };
            groups.a = 42;
            deleteMarker(n, "groups");
            const visible = n as unknown as RegExpMatchArray;
            if (visible.groups !== undefined) return 6;
            return 0;
        }
    "#;
    assert_eq!(run_with_delete(source, 3), "0");
}

fn run_with_delete(source: &str, expected_nodes: usize) -> String {
    let source = format!(
        "function deleteMarker(o: unknown, key: string): boolean {{ return true; }}\n{source}"
    );
    run(&source, move |ast| {
        let mut count = 0;
        for id in ast.expr_ids().unwrap() {
            let expr = ast.try_expr(id).unwrap().clone();
            let TypedExprKind::Call { mangled, args, .. } = expr.kind else {
                continue;
            };
            if mangled != submilli_engine::mangle::package_symbol("main", "deleteMarker") {
                continue;
            }
            let TypedExprKind::String(name) = &ast.try_expr(args[1]).unwrap().kind else {
                panic!("literal key");
            };
            ast.try_expr_mut(id).unwrap().kind = TypedExprKind::DeleteField {
                receiver: args[0],
                name: Ident {
                    name: name.clone(),
                    span: expr.span,
                },
            };
            count += 1;
        }
        assert_eq!(count, expected_nodes);
    })
}
