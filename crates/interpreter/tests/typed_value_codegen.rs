//! Alternate front ends supply these valid TypeScript forms directly. The engine
//! front end rejects several of them, so marker calls let it build the surrounding
//! tree; each test then replaces only that marker with the reported TypedAst.
use submilli_engine::{
    BinOp, FileId, Ident, RuntimeConfig, Type, TypedArrayElement, TypedAst, TypedExpr,
    TypedExprKind, TypedObjectMember, TypedStmtKind, UnOp,
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
fn constructs_through_a_union_of_constructors() {
    // Replace the source constructs with markers so the engine front end can
    // build the surrounding tree. Use an empty array for the spread whose
    // empty tuple type the engine parser cannot spell. In makeBytes the
    // dynamic receiver stays unknown, as with TypeScript's erased cast.
    let source = include_str!("typed_value_sources/construct-union.ts")
        .replace("new c()", "constructMarker(c)")
        .replace("new c(\"x\")", "patternMarker(c, \"x\")")
        .replace("new c(...empty())", "spreadMarker(c, empty())")
        .replace("empty(): []", "empty(): number[]")
        .replace(
            "new (c as typeof Uint8Array)(size())",
            "bytesMarker(c, size())",
        );
    let source = format!(
        "function constructMarker(c: typeof TextEncoder | typeof TextDecoder): unknown {{ return c; }}
         function patternMarker(c: typeof RegExp | typeof TextEncoder, pattern: string): unknown {{ return c; }}
         function spreadMarker(c: typeof TextEncoder | typeof TextDecoder, empty: number[]): unknown {{ return c; }}
         function bytesMarker(c: unknown, size: number): Uint8Array {{ return new Uint8Array(size); }}
         {source}"
    );
    let (sources, _) = submilli_engine::Sources::single("construct-union.ts", &source).unwrap();
    assert_eq!(
        run(&source, move |ast| {
            let mut count = 0;
            for id in ast.expr_ids().unwrap() {
                let expr = ast.try_expr_mut(id).unwrap();
                let TypedExprKind::Call { mangled, args, .. } = &expr.kind else {
                    continue;
                };
                let (constructor_args, authored) = match mangled.as_str() {
                    "main#constructMarker" => (vec![], vec![]),
                    "main#patternMarker" | "main#bytesMarker" => {
                        (vec![args[1]], vec![TypedArrayElement::Value(args[1])])
                    }
                    "main#spreadMarker" => (vec![], vec![TypedArrayElement::Spread(args[1])]),
                    _ => continue,
                };
                let span = expr.span;
                expr.kind = TypedExprKind::MethodCall {
                    receiver: args[0],
                    iface: submilli_engine::mangle::prelude("Object"),
                    member: submilli_engine::Member::ConstructSignature(span),
                    args: constructor_args,
                    type_predicate: None,
                };
                ast.record_authored_arguments(span, authored);
                count += 1;
            }
            assert_eq!(count, 4);
            assert_dynamic_constructor_edges(ast, &sources);
        }),
        "3"
    );
}

#[test]
fn dynamically_constructs_a_namespaced_constructor() {
    let source = r#"
        function main(): number {
            const c = Temporal.Duration;
            if (String(c) !== "function Duration() { [native code] }") return -1;
            return (new c({ hours: 3 })).hours;
        }
    "#;
    assert_eq!(run(source, |_| {}), "3");
    assert_eq!(
        run(source, |ast| {
            let mut count = 0;
            for id in ast.expr_ids().unwrap() {
                if let TypedExprKind::MethodCall {
                    iface,
                    member: submilli_engine::Member::ConstructSignature(_),
                    ..
                } = &mut ast.try_expr_mut(id).unwrap().kind
                {
                    *iface = submilli_engine::mangle::prelude("Object");
                    count += 1;
                }
            }
            assert_eq!(count, 1);
        }),
        "3"
    );
}

fn assert_dynamic_constructor_edges(ast: &TypedAst, sources: &submilli_engine::Sources) {
    let (prelude, host, internal) =
        submilli_engine::runtime::prelude::cached_runtime_package_declarations();
    let declaration = submilli_engine::PackageDeclaration::with_package("main");
    let authority = submilli_engine::authority::analyse(
        &declaration,
        ast,
        sources,
        prelude.iter().chain(host).chain(internal),
    )
    .unwrap();
    for caller in ["main#make", "main#makeBytes"] {
        assert!(authority.edges.iter().any(|edge| {
            edge.caller == caller
                && edge.unresolved
                && edge.target.is_none()
                && edge.reason.as_deref() == Some("dynamic constructor target")
        }));
    }
}

#[test]
fn array_method_write_reports_a_typecheck_error() {
    let source = include_str!("typed_value_sources/array-method-write.ts")
        .replace("values.push = replacement", "write(values, replacement)");
    let source =
        format!("function write(a: number[], f: (n: number) => number): void {{}}\n{source}");
    submilli_engine::compiler_thread::run(|| {
        let (mut ast, diagnostics) =
            submilli_engine::compile::typecheck_to_typed_ast(&source, FileId(0));
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(replace_field_write_markers(&mut ast, "push"), 1);
        let diagnostics = submilli_engine::typechecker::rules::check_script_submilli(&ast, &[])
            .expect("check array method write");
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.severity == submilli_engine::Severity::Error
                    && diagnostic
                        .message
                        .contains("cannot assign to named property `push` on an array")
            }),
            "{diagnostics:?}"
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn tuple_length_write_uses_array_setter() {
    let source = include_str!("typed_value_sources/tuple-length-write.ts")
        .replace("named.length = 1", "write(named, 1)")
        .replace("tuple[\"length\"] = size", "write(tuple, size)");
    let source = format!("function write(a: [number, number?], n: number): void {{}}\n{source}");
    assert_eq!(
        run(&source, |ast| {
            assert_eq!(replace_field_write_markers(ast, "length"), 2);
        }),
        "0"
    );
}

fn replace_field_write_markers(ast: &mut TypedAst, name: &str) -> usize {
    let marker = submilli_engine::mangle::package_symbol("main", "write");
    let mut replaced = 0;
    for id in ast.stmt_ids().unwrap() {
        let stmt = ast.try_stmt(id).unwrap();
        let TypedStmtKind::Expr(expr) = stmt.kind else {
            continue;
        };
        let TypedExprKind::Call { mangled, args, .. } = &ast.try_expr(expr).unwrap().kind else {
            continue;
        };
        if mangled != &marker {
            continue;
        }
        ast.try_stmt_mut(id).unwrap().kind = TypedStmtKind::AssignField {
            receiver: args[0],
            value: args[1],
            name: Ident {
                name: name.into(),
                span: stmt.span,
            },
        };
        replaced += 1;
    }
    replaced
}

#[test]
fn concatenation_can_have_a_template_literal_type() {
    let source =
        include_str!("typed_value_sources/template-concatenation.ts").replace(": `k${number}`", "");
    assert_eq!(
        run(&source, |ast| {
            let mut replaced = 0;
            for id in ast.expr_ids().unwrap() {
                let expr = ast.try_expr_mut(id).unwrap();
                if matches!(expr.kind, TypedExprKind::Binary { op: BinOp::Add, .. }) {
                    expr.ty = Type::TemplateLiteral {
                        parts: vec!["k".into(), "".into()],
                        substitutions: vec![Type::Number],
                    };
                    replaced += 1;
                }
            }
            assert_eq!(replaced, 1);
        }),
        "k3"
    );
}

#[test]
fn logical_and_preserves_falsy_generic_intersections() {
    assert_eq!(
        run(
            include_str!("typed_value_sources/generic-logical-and.ts"),
            |ast| {
                let mut replaced = 0;
                for id in ast.expr_ids().unwrap() {
                    let TypedExprKind::Binary {
                        op: BinOp::And,
                        lhs,
                        rhs,
                    } = ast.try_expr(id).unwrap().kind
                    else {
                        continue;
                    };
                    let parameter = Type::TypeVar("T".into());
                    let (left, right) = if ast.try_expr(rhs).unwrap().ty == Type::String {
                        (
                            Type::union(vec![
                                Type::Intersection(vec![
                                    parameter.clone(),
                                    Type::BooleanLiteral(false),
                                ]),
                                Type::Intersection(vec![parameter, Type::BooleanLiteral(true)]),
                            ]),
                            Type::String,
                        )
                    } else {
                        (
                            Type::Intersection(vec![parameter, Type::Number]),
                            Type::Boolean,
                        )
                    };
                    ast.try_expr_mut(lhs).unwrap().ty = left;
                    ast.try_expr_mut(id).unwrap().ty = right;
                    replaced += 1;
                }
                assert_eq!(replaced, 2);
            }
        ),
        "0"
    );
}

#[test]
fn placeholders_do_not_replace_captured_parameter_bindings() {
    assert_eq!(
        run(
            include_str!("typed_value_sources/nested-captured-parameter.ts"),
            |ast| {
                // An alternate front end can append placeholders after the real closures.
                let mut replacements = std::collections::BTreeMap::new();
                for id in std::mem::take(&mut ast.placeholder_closures) {
                    let placeholder = ast.try_expr(id).unwrap().clone();
                    let new_id = ast.try_push_expr(placeholder).unwrap();
                    ast.placeholder_closures.extend([id, new_id]);
                    replacements.insert(id, new_id);
                }
                assert_eq!(replacements.len(), 3);
                for id in ast.stmt_ids().unwrap() {
                    if let TypedStmtKind::Let { value, .. } =
                        &mut ast.try_stmt_mut(id).unwrap().kind
                        && let Some(replacement) = replacements.get(value)
                    {
                        *value = *replacement;
                    }
                }
            }
        ),
        "0"
    );
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

#[test]
fn forward_capture_reads_later_const() {
    // Infer the same binding types in declaration order, then restore the
    // authored order at the alternate-frontend boundary.
    let source = include_str!("typed_value_sources/forward-capture.ts");
    let infer_source = source.replace(
        "const read = (): number => late;\n  const late = 7;",
        "const late = 7;\n  const read = (): number => late;",
    );
    assert_eq!(
        run(&infer_source, |ast| {
            let body = ast
                .functions
                .iter()
                .find(|f| f.name.name == "main")
                .unwrap()
                .body;
            let TypedStmtKind::Block(statements) = &mut ast.try_stmt_mut(body).unwrap().kind else {
                panic!("main block")
            };
            statements.swap(0, 1);
        }),
        "7"
    );
}

#[test]
fn forward_capture_tdz_shadowing_and_block_reentry() {
    let source = include_str!("typed_value_sources/forward-capture-edges.ts")
        .replace("=> late", "=> lateMarker()")
        .replace("read ? late :", "read ? lateMarker() :");
    let source = format!("function lateMarker(): number {{ return -999; }}\n{source}");
    assert_eq!(
        run(&source, |ast| {
            let mut count = 0;
            for id in ast.expr_ids().unwrap() {
                let expr = ast.try_expr_mut(id).unwrap();
                if matches!(&expr.kind, TypedExprKind::Call { mangled, .. } if mangled == &submilli_engine::mangle::package_symbol("main", "lateMarker"))
                {
                    expr.kind = TypedExprKind::LocalRef {
                        ident: Ident {
                            name: "late".into(),
                            span: expr.span,
                        },
                        boxed: false,
                    };
                    count += 1;
                }
            }
            assert_eq!(count, 5);
        }),
        "0"
    );
}

#[test]
fn forward_capture_writes_check_tdz_after_rhs() {
    let source = include_str!("typed_value_sources/forward-capture-writes.ts")
        .replace("late = value()", "writeMarker(value())")
        .replace("=> late++", "=> bumpMarker()");
    let source = format!(
        "function writeMarker(n: number): void {{}}\nfunction bumpMarker(): number {{ return 0; }}\n{source}"
    );
    assert_eq!(
        run(&source, |ast| {
            let mut writes = 0;
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
                if mangled == submilli_engine::mangle::package_symbol("main", "writeMarker") {
                    ast.try_stmt_mut(id).unwrap().kind = TypedStmtKind::AssignLocal {
                        ident: Ident {
                            name: "late".into(),
                            span: stmt.span,
                        },
                        target_ty: Type::Number,
                        value: args[0],
                        boxed: false,
                        narrowed_shadow_ty: None,
                    };
                    writes += 1;
                }
            }
            let mut bumps = 0;
            for id in ast.expr_ids().unwrap() {
                let expr = ast.try_expr_mut(id).unwrap();
                if matches!(&expr.kind, TypedExprKind::Call { mangled, .. } if mangled == &submilli_engine::mangle::package_symbol("main", "bumpMarker"))
                {
                    expr.kind = TypedExprKind::PostfixUnary {
                        op: submilli_engine::PostfixOp::Inc,
                        target: submilli_engine::PostfixTarget::Local {
                            ident: Ident {
                                name: "late".into(),
                                span: expr.span,
                            },
                            target_ty: Type::Number,
                            boxed: false,
                        },
                    };
                    bumps += 1;
                }
            }
            assert_eq!((writes, bumps), (1, 1));
        }),
        "0"
    );
}

#[test]
fn forward_capture_mutual_and_let_self_recursion() {
    let source = include_str!("typed_value_sources/forward-capture-recursion.ts")
        .replace("odd(n - 1)", "oddMarker()(n - 1)")
        .replace("fib(n - 1)", "fibMarker()(n - 1)")
        .replace("fib(n - 2)", "fibMarker()(n - 2)");
    let source = format!(
        "function oddMarker(): (n: number) => boolean {{ return (n: number): boolean => false; }}\nfunction fibMarker(): (n: number) => number {{ return (n: number): number => 0; }}\n{source}"
    );
    assert_eq!(
        run(&source, |ast| {
            let mut count = 0;
            for id in ast.expr_ids().unwrap() {
                let expr = ast.try_expr_mut(id).unwrap();
                let TypedExprKind::Call { mangled, .. } = &expr.kind else {
                    continue;
                };
                let name = if mangled
                    == &submilli_engine::mangle::package_symbol("main", "oddMarker")
                {
                    "odd"
                } else if mangled == &submilli_engine::mangle::package_symbol("main", "fibMarker") {
                    "fib"
                } else {
                    continue;
                };
                expr.kind = TypedExprKind::LocalRef {
                    ident: Ident {
                        name: name.into(),
                        span: expr.span,
                    },
                    boxed: false,
                };
                count += 1;
            }
            assert_eq!(count, 3);
        }),
        "0"
    );
}

/// Replaces each `site(n)` marker call with site `n`'s `TemplateObject`.
fn replace_template_sites(ast: &mut TypedAst, sites: &[(Vec<Option<&str>>, Vec<&str>)]) -> usize {
    let mut replaced = 0;
    for id in ast.expr_ids().unwrap() {
        let TypedExprKind::Call { mangled, args, .. } = &ast.try_expr(id).unwrap().kind else {
            continue;
        };
        if mangled.as_str() != "main#site" {
            continue;
        }
        let TypedExprKind::Number(n) = ast.try_expr(args[0]).unwrap().kind else {
            panic!("a site marker takes a number literal");
        };
        let (cooked, raw) = &sites[n as usize - 1];
        let expr = ast.try_expr_mut(id).unwrap();
        expr.kind = TypedExprKind::TemplateObject {
            cooked: cooked.iter().map(|text| text.map(str::to_owned)).collect(),
            raw: raw.iter().map(|text| (*text).to_owned()).collect(),
        };
        expr.ty = Type::Readonly(Box::new(Type::Array(Box::new(Type::String))));
        replaced += 1;
    }
    replaced
}

#[test]
fn template_objects_are_frozen_per_site_arrays_with_a_hidden_raw() {
    let source = include_str!("typed_value_sources/tagged-template.ts");
    let strings = (
        vec![Some("a"), Some("b\n"), Some("")],
        vec!["a", "b\\n", ""],
    );
    assert_eq!(
        run(source, move |ast| {
            assert_eq!(replace_template_sites(ast, &[strings.clone(), strings]), 2);
        }),
        [
            "true",
            "false",
            "a1b\n2",
            "3",
            r#"["a","b\n",""]"#,
            "0,1,2",
            "0,1,2",
            "3",
            "true",
            "true",
            r#"["a","b\\n",""]"#,
            "missing",
            "true",
            "TypeError",
            "TypeError",
            "TypeError",
            "TypeError",
            "TypeError",
            "TypeError",
            "TypeError",
            "TypeError",
            "TypeError",
            "wrote",
            "wrote",
            "TypeError",
            "TypeError",
            r#"["a","b\n",""]"#,
        ]
        .join("|")
    );
}

#[test]
fn template_objects_are_one_object_per_node() {
    let source = include_str!("typed_value_sources/tagged-template-sites.ts");
    assert_eq!(
        run(source, |ast| {
            let sites = [
                (vec![Some("a")], vec!["a"]),
                (vec![None, Some("x")], vec!["\\unicode", "x"]),
                (vec![Some("a"), Some("b")], vec!["a", "b"]),
            ];
            assert_eq!(replace_template_sites(ast, &sites), 4);
            let mut deletes = 0;
            for id in ast.expr_ids().unwrap() {
                let expr = ast.try_expr(id).unwrap().clone();
                let TypedExprKind::Call { mangled, args, .. } = expr.kind else {
                    continue;
                };
                if mangled.as_str() != "main#deleteMarker" {
                    continue;
                }
                ast.try_expr_mut(id).unwrap().kind = TypedExprKind::DeleteField {
                    receiver: args[0],
                    name: Ident {
                        name: "raw".into(),
                        span: expr.span,
                    },
                };
                deletes += 1;
            }
            assert_eq!(deletes, 1);
        }),
        r#"true|true|false|true|[null,"x"]|>a7b|TypeError|true"#
    );
}

#[test]
fn template_objects_reject_writes_where_javascript_does() {
    let source = include_str!("typed_value_sources/tagged-template-writes.ts");
    assert_eq!(
        run(source, |ast| {
            let sites = [
                (vec![Some("a")], vec!["a"]),
                (vec![Some("a"), Some("b"), Some("c")], vec!["a", "b", "c"]),
                (vec![None, Some("x")], vec!["\\u", "x"]),
            ];
            assert_eq!(replace_template_sites(ast, &sites), 3);
        }),
        [
            "wrote",
            "wrote",
            "TypeError",
            "TypeError",
            "TypeError",
            "TypeError",
            "TypeError",
            "TypeError",
            "TypeError",
            "TypeError",
            "a,b,c,x/y,b,c",
            "true",
            "undefined!x",
            "TypeError",
            r#"["a","b","c"]"#,
            "TypeError",
            "false",
            "false",
            r#"["a","b","c"]"#,
        ]
        .join("|")
    );
}

#[test]
fn calls_a_saved_closure_with_an_explicit_receiver() {
    let source = include_str!("typed_value_sources/closure-receiver.ts")
        .replace("Reflect.apply(f, receiver, [x])", "f(x)")
        .replace("Reflect.apply(f, receiver, [])", "f()");
    assert_eq!(
        run(&source, |ast| {
            let mut replaced = 0;
            for id in ast.expr_ids().unwrap() {
                let expr = ast.try_expr_mut(id).unwrap();
                if let TypedExprKind::Call { mangled, args, .. } = &expr.kind
                    && matches!(mangled.as_str(), "main#applyMarker" | "main#nullMarker")
                {
                    expr.kind = TypedExprKind::CallClosure {
                        callee: args[0],
                        receiver: Some(args[1]),
                        args: args[2..].to_vec(),
                        type_args: vec![],
                    };
                    replaced += 1;
                }
            }
            assert_eq!(replaced, 5);
        }),
        "1231442"
    );
}

#[test]
fn super_get_and_super_set_dispatch_base_accessors() {
    let source = include_str!("typed_value_sources/super-accessors.ts")
        .replace("override ", "")
        .replace("(this: Base) => number", "() => number")
        .replace(
            "return super.x * 10;",
            "return superGetNumber(\"Base\", \"x\") * 10;",
        )
        .replace("super.x = v + 1;", "superSetNumber(\"Base\", \"x\", v + 1);")
        .replace(
            "() => () => super.x;",
            "() => () => superGetNumber(\"Base\", \"x\");",
        )
        .replace("() => super.x;", "() => superGetNumber(\"Base\", \"x\");")
        .replace(
            "const assigned = (super.x = v);",
            "const assigned = superAssignNumber(\"Base\", \"x\", v);",
        )
        .replace("super.x = v;", "superSetNumber(\"Base\", \"x\", v);")
        .replace(
            "return super.reader();",
            "return superGetReader(\"Base\", \"reader\")();",
        )
        .replace(
            "return String(super.pureNum) + \":\" + String(super.pureFlag);",
            "return String(superGetNumber(\"Base\", \"pureNum\")) + \":\" + String(superGetBool(\"Base\", \"pureFlag\"));",
        )
        .replace(
            "super.x += 3;",
            "superSetNumber(\"Base\", \"x\", superGetNumber(\"Base\", \"x\") + 3);",
        )
        .replace(
            "const prev = super.x++;",
            "const prev = superGetNumber(\"Base\", \"x\"); superSetNumber(\"Base\", \"x\", prev + 1);",
        )
        .replace(
            "super.maybe ??= fallback();",
            "if (superGetOpt(\"Base\", \"maybe\") === undefined) { superSetOpt(\"Base\", \"maybe\", fallback()); }",
        )
        .replace(
            "return super.maybe ?? -1;",
            "return superGetOpt(\"Base\", \"maybe\") ?? -1;",
        )
        .replace(
            "return super.x + 1000;",
            "return superGetNumber(\"Mid\", \"x\") + 1000;",
        )
        .replace("super.x = v * 2;", "superSetNumber(\"Mid\", \"x\", v * 2);")
        .replace(
            "super.maybe = v;",
            "superSetOpt(\"Base\", \"maybe\", v);",
        )
        .replace(
            "super.value = super.value + 5;",
            "superSetNumber(\"Box\", \"value\", superGetNumber(\"Box\", \"value\") + 5);",
        )
        .replace(
            "return super.value * 3;",
            "return superGetNumber(\"Box\", \"value\") * 3;",
        )
        .replace(
            "super.value = v - 1;",
            "superSetNumber(\"Box\", \"value\", v - 1);",
        )
        .replace(
            "const inner = super.value;",
            "const inner = superGetObj(\"Box\", \"value\");",
        )
        .replace(
            "super.value = { a: v.a * 2 };",
            "superSetObj(\"Box\", \"value\", { a: v.a * 2 });",
        );
    let source = format!(
        "function superGetNumber(owner: string, name: string): number {{ return 0; }}
         function superGetBool(owner: string, name: string): boolean {{ return false; }}
         function superGetReader(owner: string, name: string): () => number {{ return () => 0; }}
         function superSetNumber(owner: string, name: string, value: number): void {{}}
         function superAssignNumber(owner: string, name: string, value: number): number {{ return value; }}
         function superGetOpt(owner: string, name: string): number | undefined {{ return undefined; }}
         function superSetOpt(owner: string, name: string, value: number | undefined): void {{}}
         function superGetObj(owner: string, name: string): {{ a: number }} {{ return {{ a: 0 }}; }}
         function superSetObj(owner: string, name: string, value: {{ a: number }}): void {{}}
         {source}"
    );
    let (sources, _) = submilli_engine::Sources::single("super-accessors.ts", &source).unwrap();
    assert_eq!(
        run(&source, move |ast| {
            let (gets, sets) = replace_super_accessor_markers(ast, "main");
            assert_eq!((gets, sets), (14, 12));

            let diagnostics = submilli_engine::typechecker::rules::check_script_submilli(ast, &[])
                .expect("rules check super accessors");
            assert!(diagnostics.is_empty(), "{diagnostics:?}");

            let (prelude, host, internal) =
                submilli_engine::runtime::prelude::cached_runtime_package_declarations();
            let declaration = submilli_engine::PackageDeclaration::with_package("main");
            let authority = submilli_engine::authority::analyse(
                &declaration,
                ast,
                &sources,
                prelude.iter().chain(host).chain(internal),
            )
            .unwrap();
            assert!(
                authority.edges.iter().any(|edge| {
                    edge.caller == "main#Mid#get#x"
                        && edge.target.as_deref() == Some("main#Base#get#x")
                        && !edge.unresolved
                }),
                "expected direct SuperGet edge: {:?}",
                authority.edges
            );
            assert!(
                authority.edges.iter().any(|edge| {
                    edge.caller == "main#Mid#set#x"
                        && edge.target.as_deref() == Some("main#Base#set#x")
                        && !edge.unresolved
                }),
                "expected direct SuperSet edge: {:?}",
                authority.edges
            );
            assert!(
                !authority.edges.iter().any(|edge| {
                    edge.caller.starts_with("main#Mid#")
                        && edge.unresolved
                        && edge.reason.as_deref()
                            == Some("virtual accessor target may be overridden")
                }),
                "super accessor must not be marked virtual/unresolved: {:?}",
                authority.edges
            );
        }),
        "20|5|100|10|10|7|8|7|7|42:true|34|35|42|42|1|1020|7|1140|123|45|57|6|15"
    );
}

#[test]
fn super_get_unwidened_primitive_return_coercions() {
    // Getters that do not read a field keep unwidened primitive slots (`f64`
    // and `i32`), exercising every boxing/coercion branch in `emit_slot_return_cast`.
    let source = r#"
        function superGetNumUnion(owner: string, name: string): number | string { return 0; }
        function superGetBoolUnion(owner: string, name: string): boolean | undefined { return undefined; }
        function superGetUnknown(owner: string, name: string): unknown { return 0; }
        function superGetVoid(owner: string, name: string): void {}
        class Base {
            get n(): number { return 42; }
            get flag(): boolean { return true; }
        }
        class Sub extends Base {
            asUnion(): string {
                const a = superGetNumUnion("Base", "n");
                const b = superGetBoolUnion("Base", "flag");
                return String(a) + "|" + String(b);
            }
            asUnknown(): string {
                const a = superGetUnknown("Base", "n");
                const b = superGetUnknown("Base", "flag");
                return String(a) + "|" + String(b);
            }
            asVoid(): void {
                return superGetVoid("Base", "n");
            }
        }
        function main(): string {
            const s = new Sub();
            s.asVoid();
            return s.asUnion() + "|" + s.asUnknown();
        }
    "#;
    assert_eq!(
        run(source, |ast| {
            let mut replaced = 0;
            for id in ast.expr_ids().unwrap() {
                let expr = ast.try_expr(id).unwrap().clone();
                let TypedExprKind::Call { mangled, args, .. } = &expr.kind else {
                    continue;
                };
                if !matches!(
                    mangled.as_str(),
                    "main#superGetNumUnion"
                        | "main#superGetBoolUnion"
                        | "main#superGetUnknown"
                        | "main#superGetVoid"
                ) {
                    continue;
                }
                let TypedExprKind::String(owner_name) = &ast.try_expr(args[0]).unwrap().kind else {
                    panic!("owner marker must be a string literal");
                };
                let TypedExprKind::String(prop_name) = &ast.try_expr(args[1]).unwrap().kind else {
                    panic!("property marker must be a string literal");
                };
                ast.try_expr_mut(id).unwrap().kind = TypedExprKind::SuperGet {
                    owner: submilli_engine::mangle::package_symbol("main", owner_name),
                    name: Ident {
                        name: prop_name.clone(),
                        span: expr.span,
                    },
                };
                replaced += 1;
            }
            assert_eq!(replaced, 5);
        }),
        "42|true|42|true"
    );
}

#[test]
fn super_get_and_super_set_across_package_boundary() {
    let base_pkg = submilli_engine::compile::compile_package_checked(
        "@test/base",
        submilli_engine::ModulePath::from("lib"),
        &[submilli_engine::compile::PackageSourceModule {
            path: submilli_engine::ModulePath::from("lib"),
            source: "
                /** Grandparent class with an inherited accessor. */
                export class GrandBase {
                    private g: number = 100;
                    get gx(): number { return this.g; }
                    set gx(v: number) { this.g = v; }
                }
                /** Base class extending GrandBase with its own accessor. */
                export class Base extends GrandBase {
                    private v: number = 10;
                    get x(): number { return this.v; }
                    set x(v: number) { this.v = v; }
                }
            ",
        }],
        &[],
    )
    .expect("compile base package");

    let source = r#"
        import { Base } from "@test/base";
        function superGetNumber(owner: string, name: string): number { return 0; }
        function superSetNumber(owner: string, name: string, value: number): void {}
        class Sub extends Base {
            get x(): number {
                return superGetNumber("Base", "x") * 4;
            }
            set x(v: number) {
                superSetNumber("Base", "x", v + 3);
            }
            get gx(): number {
                return superGetNumber("GrandBase", "gx") + 1;
            }
            set gx(v: number) {
                superSetNumber("GrandBase", "gx", v * 2);
            }
        }
        function main(): string {
            const s = new Sub();
            const first = s.x;
            s.x = 5;
            const firstG = s.gx;
            s.gx = 25;
            return String(first) + "|" + String(s.x) + "|" + String(firstG) + "|" + String(s.gx);
        }
    "#;
    let output = std::thread::Builder::new()
        .stack_size(submilli_engine::compiler_limits::COMPILER_STACK_BYTES)
        .spawn(move || {
            let (prelude, host, internal) =
                submilli_engine::runtime::prelude::cached_runtime_package_declarations();
            let stdlib = submilli_engine::runtime::stdlib_package_declarations();
            let mut deps = vec![&base_pkg.declaration];
            deps.extend(stdlib.iter());
            let request = submilli_engine::script_request(&deps);
            let checked = submilli_engine::FrontEnd::parse_script(
                &submilli_engine::EngineFrontEnd,
                source.to_owned(),
                FileId(0),
            )
            .check(&request)
            .unwrap();
            assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
            let mut ast = checked.typed_ast;

            let (gets, sets) = replace_super_accessor_markers(&mut ast, "@test/base");
            assert_eq!((gets, sets), (2, 2));

            let (sources, _) =
                submilli_engine::Sources::single("cross-pkg-super.ts", source).unwrap();
            let declaration = submilli_engine::PackageDeclaration::with_package("main");
            let authority = submilli_engine::authority::analyse(
                &declaration,
                &ast,
                &sources,
                prelude
                    .iter()
                    .chain(host)
                    .chain(internal)
                    .chain(std::iter::once(&base_pkg.declaration)),
            )
            .unwrap();
            for caller in [
                "main#Sub#get#x",
                "main#Sub#set#x",
                "main#Sub#get#gx",
                "main#Sub#set#gx",
            ] {
                assert!(
                    authority.edges.iter().any(|edge| {
                        edge.caller == caller
                            && edge.unresolved
                            && edge.reason.as_deref()
                                == Some("external accessor body is not locally analyzable")
                    }),
                    "expected external accessor edge for {caller}: {:?}",
                    authority.edges
                );
            }

            let ast = submilli_engine::desugar(submilli_engine::capture(ast).unwrap(), FileId(0))
                .unwrap();
            let all_deps: Vec<_> = prelude
                .iter()
                .chain(host)
                .chain(internal)
                .chain(std::iter::once(&base_pkg.declaration))
                .collect();
            let generated = submilli_engine::codegen::codegen_with_type_info(
                source,
                "cross-pkg-super.ts",
                FileId(0),
                &ast,
                &all_deps,
            )
            .unwrap();
            let cfg = RuntimeConfig::default();
            let engine = cfg.engine().unwrap();
            let mut data = submilli_engine::runtime::StoreData::with_tempdir().unwrap();
            data.install_type_info(generated.type_info);
            let mut store = cfg.store_async(&engine, data).unwrap();
            let mut linker = wasmtime::Linker::new(&engine);
            let base_module = wasmtime::Module::new(&engine, &base_pkg.wasm).unwrap();
            pollster::block_on(async {
                submilli_engine::runtime::install_runtime_async(&mut linker, &mut store)
                    .await
                    .unwrap();
                submilli_engine::runtime::install_package_modules_async(
                    &mut linker,
                    &mut store,
                    &[submilli_engine::runtime::LinkedPackageModule {
                        module: &base_module,
                        declaration: &base_pkg.declaration,
                        type_info: &base_pkg.type_info,
                    }],
                )
                .await
                .unwrap();
                let module = wasmtime::Module::new(&engine, &generated.wasm).unwrap();
                let inst = submilli_engine::instantiate_program_async(&linker, &mut store, &module)
                    .await
                    .unwrap();
                submilli_engine::dispatch_main_async(&mut store, &inst)
                    .await
                    .unwrap()
                    .unwrap()
            })
        })
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(output, "40|32|101|51");
}

fn replace_super_accessor_markers(ast: &mut TypedAst, package: &str) -> (usize, usize) {
    let mut sets = 0;
    for id in ast.stmt_ids().unwrap() {
        let stmt = ast.try_stmt(id).unwrap().clone();
        let TypedStmtKind::Expr(expr_id) = stmt.kind else {
            continue;
        };
        let TypedExprKind::Call { mangled, args, .. } = &ast.try_expr(expr_id).unwrap().kind else {
            continue;
        };
        if !matches!(
            mangled.as_str(),
            "main#superSetNumber" | "main#superSetOpt" | "main#superSetObj"
        ) {
            continue;
        }
        let TypedExprKind::String(owner_name) = &ast.try_expr(args[0]).unwrap().kind else {
            panic!("owner marker must be a string literal");
        };
        let TypedExprKind::String(prop_name) = &ast.try_expr(args[1]).unwrap().kind else {
            panic!("property marker must be a string literal");
        };
        let owner = submilli_engine::mangle::package_symbol(package, owner_name);
        let prop = Ident {
            name: prop_name.clone(),
            span: stmt.span,
        };
        let value = args[2];
        ast.try_stmt_mut(id).unwrap().kind = TypedStmtKind::SuperSet {
            owner,
            name: prop,
            value,
        };
        sets += 1;
    }

    let mut gets = 0;
    for id in ast.expr_ids().unwrap() {
        let expr = ast.try_expr(id).unwrap().clone();
        let TypedExprKind::Call { mangled, args, .. } = &expr.kind else {
            continue;
        };
        if mangled.as_str() == "main#superAssignNumber" {
            let TypedExprKind::String(owner_name) = &ast.try_expr(args[0]).unwrap().kind else {
                panic!("owner marker must be a string literal");
            };
            let TypedExprKind::String(prop_name) = &ast.try_expr(args[1]).unwrap().kind else {
                panic!("property marker must be a string literal");
            };
            let owner = submilli_engine::mangle::package_symbol(package, owner_name);
            let prop = Ident {
                name: prop_name.clone(),
                span: expr.span,
            };
            let value = args[2];
            let tmp_name = Ident {
                name: format!("$super_assign_{sets}"),
                span: expr.span,
            };
            let let_stmt = ast
                .try_push_stmt(submilli_engine::TypedStmt {
                    kind: TypedStmtKind::Const {
                        name: tmp_name.clone(),
                        ty: Type::Number,
                        value,
                        doc: None,
                    },
                    span: expr.span,
                })
                .unwrap();
            let tmp_read_for_set = ast
                .try_push_expr(TypedExpr {
                    kind: TypedExprKind::LocalRef {
                        ident: tmp_name.clone(),
                        boxed: false,
                    },
                    span: expr.span,
                    ty: Type::Number,
                })
                .unwrap();
            let set_stmt = ast
                .try_push_stmt(submilli_engine::TypedStmt {
                    kind: TypedStmtKind::SuperSet {
                        owner,
                        name: prop,
                        value: tmp_read_for_set,
                    },
                    span: expr.span,
                })
                .unwrap();
            let tmp_read_for_result = ast
                .try_push_expr(TypedExpr {
                    kind: TypedExprKind::LocalRef {
                        ident: tmp_name,
                        boxed: false,
                    },
                    span: expr.span,
                    ty: Type::Number,
                })
                .unwrap();
            ast.try_expr_mut(id).unwrap().kind = TypedExprKind::Sequence {
                stmts: vec![let_stmt, set_stmt],
                result: tmp_read_for_result,
            };
            sets += 1;
            continue;
        }
        if !matches!(
            mangled.as_str(),
            "main#superGetNumber"
                | "main#superGetBool"
                | "main#superGetReader"
                | "main#superGetOpt"
                | "main#superGetObj"
        ) {
            continue;
        }
        let TypedExprKind::String(owner_name) = &ast.try_expr(args[0]).unwrap().kind else {
            panic!("owner marker must be a string literal");
        };
        let TypedExprKind::String(prop_name) = &ast.try_expr(args[1]).unwrap().kind else {
            panic!("property marker must be a string literal");
        };
        let owner = submilli_engine::mangle::package_symbol(package, owner_name);
        let prop = Ident {
            name: prop_name.clone(),
            span: expr.span,
        };
        ast.try_expr_mut(id).unwrap().kind = TypedExprKind::SuperGet { owner, name: prop };
        gets += 1;
    }

    (gets, sets)
}

#[test]
fn super_method_call_reaches_global_prototype_to_string() {
    let source = include_str!("typed_value_sources/super-prototype-to-string.ts")
        .replace("override ", "")
        .replace(
            "\"wrapped(\" + super.toString()",
            "\"wrapped(\" + superToStringMarker(\"Error\")",
        )
        .replace(
            "\"mytype(\" + super.toString()",
            "\"mytype(\" + superToStringMarker(\"TypeError\")",
        )
        .replace(
            "\"plain(\" + super.toString()",
            "\"plain(\" + superToStringMarker(\"Object\")",
        );
    let source =
        format!("function superToStringMarker(owner: string): string {{ return \"\"; }}\n{source}");
    let expected = "wrapped(MyErr: boom)|wrapped(MyErr)|sub(wrapped(MyErr: boom))|mytype(TypeError: bad)|plain([object Object])";
    let (sources, _) =
        submilli_engine::Sources::single("super-prototype-to-string.ts", &source).unwrap();
    assert_eq!(
        run(&source, move |ast| {
            let mut replaced = 0;
            for id in ast.expr_ids().unwrap() {
                let expr = ast.try_expr(id).unwrap().clone();
                let TypedExprKind::Call { mangled, args, .. } = &expr.kind else {
                    continue;
                };
                if mangled.as_str() != "main#superToStringMarker" {
                    continue;
                }
                let TypedExprKind::String(owner_name) = &ast.try_expr(args[0]).unwrap().kind else {
                    panic!("owner marker must be a string literal");
                };
                ast.try_expr_mut(id).unwrap().kind = TypedExprKind::SuperMethodCall {
                    owner: submilli_engine::mangle::prelude(owner_name),
                    name: Ident {
                        name: "toString".into(),
                        span: expr.span,
                    },
                    args: vec![],
                };
                replaced += 1;
            }
            assert_eq!(replaced, 3);

            let (prelude, host, internal) =
                submilli_engine::runtime::prelude::cached_runtime_package_declarations();
            let declaration = submilli_engine::PackageDeclaration::with_package("main");
            let authority = submilli_engine::authority::analyse(
                &declaration,
                ast,
                &sources,
                prelude.iter().chain(host).chain(internal),
            )
            .unwrap();
            assert!(
                !authority.edges.iter().any(|edge| {
                    edge.unresolved
                        && edge.reason.as_deref() == Some("unresolved super-method target")
                }),
                "built-in prototype toString should resolve in authority: {:?}",
                authority.edges
            );
        }),
        expected
    );

    // Extra arguments are evaluated for their side effects and dropped, and
    // widened `unknown` return types stay valid.
    let extra_source = r#"
        let trace = 0;
        function bump(): number { trace = trace + 1; return trace; }
        function superToStringWithArg(arg: number): unknown { return ""; }
        class MyErr extends Error {
            toString(): string {
                return String(superToStringWithArg(bump()));
            }
        }
        function main(): string {
            return new MyErr("ok").toString() + "|" + String(trace);
        }
    "#;
    assert_eq!(
        run(extra_source, |ast| {
            let mut replaced = 0;
            for id in ast.expr_ids().unwrap() {
                let expr = ast.try_expr(id).unwrap().clone();
                let TypedExprKind::Call { mangled, args, .. } = &expr.kind else {
                    continue;
                };
                if mangled.as_str() != "main#superToStringWithArg" {
                    continue;
                }
                ast.try_expr_mut(id).unwrap().kind = TypedExprKind::SuperMethodCall {
                    owner: submilli_engine::mangle::prelude("Error"),
                    name: Ident {
                        name: "toString".into(),
                        span: expr.span,
                    },
                    args: args.clone(),
                };
                replaced += 1;
            }
            assert_eq!(replaced, 1);
        }),
        "Error: ok|1"
    );
}

#[test]
fn super_accessor_and_prototype_invalid_targets_fail_cleanly() {
    fn compile_rewritten(
        source: &str,
        rewrite: impl FnOnce(&mut TypedAst) + Send + 'static,
    ) -> Result<(), submilli_engine::compiler_error::CompilerFailure> {
        let source = source.to_owned();
        std::thread::Builder::new()
            .stack_size(submilli_engine::compiler_limits::COMPILER_STACK_BYTES)
            .spawn(move || {
                let (mut ast, diagnostics) =
                    submilli_engine::compile::typecheck_to_typed_ast(&source, FileId(0));
                assert!(diagnostics.is_empty(), "{diagnostics:?}");
                rewrite(&mut ast);
                let ast = submilli_engine::desugar(submilli_engine::capture(ast)?, FileId(0))?;
                let (prelude, host, internal) =
                    submilli_engine::runtime::prelude::cached_runtime_package_declarations();
                let dependencies: Vec<_> = prelude.iter().chain(host).chain(internal).collect();
                submilli_engine::codegen::codegen_with_type_info(
                    &source,
                    "invalid-super.ts",
                    FileId(0),
                    &ast,
                    &dependencies,
                )
                .map(|_| ())
            })
            .unwrap()
            .join()
            .unwrap()
    }

    // SuperGet on a setter-only accessor fails cleanly as an internal error.
    let setter_only = r#"
        function mark(): number { return 0; }
        class A { private v: number = 0; set x(v: number) { this.v = v; } }
        class B extends A { read(): number { return mark(); } }
        function main(): number { return new B().read(); }
    "#;
    assert!(matches!(
        compile_rewritten(setter_only, |ast| {
            for id in ast.expr_ids().unwrap() {
                let expr = ast.try_expr_mut(id).unwrap();
                if let TypedExprKind::Call { mangled, .. } = &expr.kind
                    && mangled.as_str() == "main#mark"
                {
                    expr.kind = TypedExprKind::SuperGet {
                        owner: submilli_engine::mangle::package_symbol("main", "A"),
                        name: Ident {
                            name: "x".into(),
                            span: expr.span,
                        },
                    };
                }
            }
        }),
        Err(submilli_engine::compiler_error::CompilerFailure::Internal { .. })
    ));

    // SuperSet on a getter-only accessor fails cleanly as an internal error.
    let getter_only = r#"
        function mark(v: number): void {}
        class A { get x(): number { return 1; } }
        class B extends A { write(v: number): void { mark(v); } }
        function main(): void { new B().write(2); }
    "#;
    assert!(matches!(
        compile_rewritten(getter_only, |ast| {
            for id in ast.stmt_ids().unwrap() {
                let stmt = ast.try_stmt(id).unwrap().clone();
                if let TypedStmtKind::Expr(expr_id) = stmt.kind
                    && let TypedExprKind::Call { mangled, args, .. } =
                        &ast.try_expr(expr_id).unwrap().kind
                    && mangled.as_str() == "main#mark"
                {
                    let value = args[0];
                    ast.try_stmt_mut(id).unwrap().kind = TypedStmtKind::SuperSet {
                        owner: submilli_engine::mangle::package_symbol("main", "A"),
                        name: Ident {
                            name: "x".into(),
                            span: stmt.span,
                        },
                        value,
                    };
                }
            }
        }),
        Err(submilli_engine::compiler_error::CompilerFailure::Internal { .. })
    ));

    // SuperGet outside a class member body fails cleanly in capture/codegen.
    let outside_class = r#"
        function mark(): number { return 0; }
        class A { get x(): number { return 1; } }
        function main(): number { return mark(); }
    "#;
    assert!(matches!(
        compile_rewritten(outside_class, |ast| {
            for id in ast.expr_ids().unwrap() {
                let expr = ast.try_expr_mut(id).unwrap();
                if let TypedExprKind::Call { mangled, .. } = &expr.kind
                    && mangled.as_str() == "main#mark"
                {
                    expr.kind = TypedExprKind::SuperGet {
                        owner: submilli_engine::mangle::package_symbol("main", "A"),
                        name: Ident {
                            name: "x".into(),
                            span: expr.span,
                        },
                    };
                }
            }
        }),
        Err(submilli_engine::compiler_error::CompilerFailure::Internal { .. })
    ));

    // SuperMethodCall { owner: user class without toString method, name: "toString" }
    // must fail cleanly instead of dispatching through the user class's vtable slot 0.
    let non_declaring_user_owner = r#"
        function mark(): string { return ""; }
        class Plain {}
        class Sub extends Plain { toString(): string { return mark(); } }
        function main(): string { return new Sub().toString(); }
    "#;
    assert!(matches!(
        compile_rewritten(non_declaring_user_owner, |ast| {
            for id in ast.expr_ids().unwrap() {
                let expr = ast.try_expr_mut(id).unwrap();
                if let TypedExprKind::Call { mangled, .. } = &expr.kind
                    && mangled.as_str() == "main#mark"
                {
                    expr.kind = TypedExprKind::SuperMethodCall {
                        owner: submilli_engine::mangle::package_symbol("main", "Plain"),
                        name: Ident {
                            name: "toString".into(),
                            span: expr.span,
                        },
                        args: vec![],
                    };
                }
            }
        }),
        Err(submilli_engine::compiler_error::CompilerFailure::Internal { .. })
    ));
}

#[test]
fn field_over_prototype_shadows_base_method_and_accessor() {
    let source = include_str!("typed_value_sources/field-over-prototype.ts")
        .replace(
            "m: () => number = (): number => 2;",
            "__shadow_m: () => number = (): number => 2;",
        )
        .replace("x: number = 20;", "__shadow_x: number = 20;")
        .replace("x?: number;", "__shadow_x?: number;")
        .replace("const bound = asBase.m;", "const bound = readM(asBase);")
        .replace(
            "const spreadD = { ...d };",
            "const spreadD = { ...(d as unknown as Record<string, unknown>) };",
        )
        .replace("delete (d as { m?: () => number }).m;", "deleteM(d);")
        .replace(
            "const spreadS = { ...s };",
            "const spreadS = { ...(s as unknown as Record<string, unknown>) };",
        )
        .replace("String(spreadS.x),", r#"String(spreadS["x"]),"#);
    let source = format!(
        "function readM(_o: BaseMethod): () => number {{ return () => 0; }}\n\
         function deleteM(_o: SubMethod): boolean {{ return false; }}\n\
         {source}"
    );
    assert_eq!(
        run(&source, |ast| {
            let mut marked = 0;
            for decl in &mut ast.types {
                let submilli_engine::TypedTypeDecl::Class(class) = decl else {
                    continue;
                };
                for field in &mut class.fields {
                    if let Some(real) = field.name.name.strip_prefix("__shadow_") {
                        field.name.name = real.to_string();
                        field.over_prototype = true;
                        marked += 1;
                    }
                }
            }
            assert_eq!(marked, 3);
            let mut reads = 0;
            let mut deletes = 0;
            for id in ast.expr_ids().unwrap() {
                let expr = ast.try_expr(id).unwrap().clone();
                let TypedExprKind::Call { mangled, args, .. } = expr.kind else {
                    continue;
                };
                match mangled.as_str() {
                    "main#readM" => {
                        ast.try_expr_mut(id).unwrap().kind = TypedExprKind::FieldAccess {
                            receiver: args[0],
                            name: Ident {
                                name: "m".into(),
                                span: expr.span,
                            },
                        };
                        reads += 1;
                    }
                    "main#deleteM" => {
                        ast.try_expr_mut(id).unwrap().kind = TypedExprKind::DeleteField {
                            receiver: args[0],
                            name: Ident {
                                name: "m".into(),
                                span: expr.span,
                            },
                        };
                        deletes += 1;
                    }
                    _ => {}
                }
            }
            assert_eq!(reads, 1);
            assert_eq!(deletes, 1);
        }),
        [
            r#"1|1|2|2|2|20|2|1|1|10|baseField,beforeInit,m,post,pre|baseField,beforeInit,m,post,pre|baseField,beforeInit,m,post,pre|true|true|false|true|{"baseField":10,"beforeInit":1,"post":2,"pre":1}"#,
            "10|1|false|true",
            r#"10|20|25|25|25|get,ctor:10,get|get,ctor:10,get|log,after,before,tail,x|log,after,before,tail,x|true|true|25|{"after":20,"before":10,"log":["get","ctor:10","get"],"tail":30,"x":25}"#,
            "true|true|true|log,x",
        ]
        .join(" // ")
    );
}

#[test]
fn assign_field_creates_own_property_over_prototype_method() {
    let source = include_str!("typed_value_sources/assign-over-method.ts")
        .replace("b1.m = () => 99;", "writeM(b1, () => 99);")
        .replace("b1.m = () => 77;", "writeM(b1, () => 77);")
        .replace(
            r#"b1.toString = () => "custom-b1";"#,
            r#"writeToString(b1, () => "custom-b1");"#,
        )
        .replace(
            r#"e1.toString = () => "custom-err";"#,
            r#"writeErrToString(e1, () => "custom-err");"#,
        )
        .replace("super.m = f;", "writeSuperM(this, f);")
        .replace(
            "Object.keys({ ...b1 }).join(\",\")",
            "Object.keys({ ...(b1 as unknown as Record<string, unknown>) }).join(\",\")",
        );
    let source = format!(
        "function writeM(_o: Base, _f: () => number): void {{}}\n\
         function writeSuperM(_o: Derived, _f: () => number): void {{}}\n\
         function writeToString(_o: Base, _f: () => string): void {{}}\n\
         function writeErrToString(_o: Error, _f: () => string): void {{}}\n\
         {source}"
    );
    assert_eq!(
        run(&source, |ast| {
            let mut replaced = 0;
            for id in ast.stmt_ids().unwrap() {
                let stmt = ast.try_stmt(id).unwrap();
                let TypedStmtKind::Expr(expr) = stmt.kind else {
                    continue;
                };
                let TypedExprKind::Call { mangled, args, .. } = &ast.try_expr(expr).unwrap().kind
                else {
                    continue;
                };
                let prop = match mangled.as_str() {
                    "main#writeM" | "main#writeSuperM" => "m",
                    "main#writeToString" | "main#writeErrToString" => "toString",
                    _ => continue,
                };
                ast.try_stmt_mut(id).unwrap().kind = TypedStmtKind::AssignField {
                    receiver: args[0],
                    value: args[1],
                    name: Ident {
                        name: prop.into(),
                        span: stmt.span,
                    },
                };
                replaced += 1;
            }
            assert_eq!(replaced, 5);
        }),
        [
            "99|990|1|10|true|false|true|true|a,m|a,m|a,m",
            "770|a,m",
            r#"custom-b1|custom-b1|[object Object]|[object Object]|true|false|a,m,toString|{"a":1}"#,
            "custom-err|custom-err|Error: ok|true|false",
            "55|550|10|true|a,extra,m",
            "42|false",
            "0,2,b,a|0,2,b,a|0,2,b,a",
        ]
        .join(" // ")
    );
}

#[test]
fn accessor_half_redeclared_alone_clears_inherited_half() {
    let source = include_str!("typed_value_sources/accessor-half-redeclared.ts");
    assert_eq!(
        run(source, |_| {}),
        r#"123|10|true|true|true|true|false|{"held":10,"last":123}|99|99|99|TypeError|TypeError|TypeError|10|true|false|{"held":10,"x":99}"#
    );
}

#[test]
fn cross_package_field_over_prototype_and_method_assign() {
    use submilli_engine::FrontEnd;
    use submilli_engine::front_end::EngineFrontEnd;
    use submilli_engine::runtime::{
        LinkedPackageModule, StoreData, Vfs, install_package_modules_async, install_runtime_async,
        install_tenant_limits,
    };
    use submilli_engine::{
        ModulePath, PackageSourceModule, Sources, TypeKind, compile_package, dispatch_main_async,
    };
    use wasmtime::{Linker, Module};

    let lib_source = r#"
/** Base class with a prototype method. */
export class Base {
  a: number = 1;
  before: number = 0;

  constructor() {
    this.before = this.m();
  }

  m(): number {
    return 1;
  }

  call(): number {
    return this.m() * 10;
  }
}

/** Subclass with an over-prototype field. */
export class ExportedSub extends Base {
  __shadow_m: () => number = (): number => 2;
  tail: number = 3;
}
"#;

    let app_source = r#"
import { Base, ExportedSub } from "@test/proto";

function writeM(_o: Base, _f: () => number): void {}

class LocalGrandChild extends ExportedSub {
  gc: number = 4;
}

class LocalSub extends Base {
  __shadow_m: () => number = (): number => 5;
}

export function main(): string {
  const es = new ExportedSub();
  const esBase: Base = es;
  const gc = new LocalGrandChild();
  const ls = new LocalSub();
  const b1 = new Base();
  const b2 = new Base();
  writeM(b1, () => 99);
  return [
    String(es.before),
    String(es.call()),
    String(esBase.m()),
    Object.keys(es).join(","),
    String(gc.before),
    String(gc.call()),
    Object.keys(gc).join(","),
    String(ls.before),
    String(ls.call()),
    Object.keys(ls).join(","),
    String(b1.call()),
    String(b2.call()),
    String(Object.hasOwn(b1, "m")),
    String(Object.hasOwn(b2, "m")),
  ].join("|");
}
"#;

    let result = std::thread::Builder::new()
        .stack_size(submilli_engine::compiler_limits::COMPILER_STACK_BYTES)
        .spawn(move || {
            let (prelude, host, internal) =
                submilli_engine::runtime::prelude::cached_runtime_package_declarations();
            let runtime_deps: Vec<_> = prelude.iter().chain(host).chain(internal).collect();
            let stdlib = submilli_engine::runtime::stdlib_package_declarations();
            let stdlib_refs: Vec<_> = stdlib.iter().collect();

            // Compile `@test/proto` with `ExportedSub.m` marked `over_prototype: true`.
            let lib_modules = vec![PackageSourceModule {
                path: ModulePath::from("lib"),
                source: lib_source,
            }];
            let mut lib_decl =
                compile_package("@test/proto", ModulePath::from("lib"), &lib_modules, &[])
                    .expect("placeholder lib package compiles")
                    .declaration;
            let (mut lib_ast, lib_diags) = submilli_engine::compile::typecheck_package_to_typed_ast(
                "@test/proto",
                ModulePath::from("lib"),
                &lib_modules,
            );
            assert!(lib_diags.is_empty(), "{lib_diags:?}");
            for decl in &mut lib_ast.types {
                if let submilli_engine::TypedTypeDecl::Class(class) = decl {
                    for field in &mut class.fields {
                        if let Some(real) = field.name.name.strip_prefix("__shadow_") {
                            field.name.name = real.to_string();
                            field.over_prototype = true;
                        }
                    }
                }
            }
            for sym in lib_decl
                .types
                .values_mut()
                .chain(lib_decl.runtime_types.values_mut())
            {
                if let TypeKind::Class { fields, .. } = &mut sym.kind
                    && let Some(mut sig) = fields.remove("__shadow_m")
                {
                    sig.over_prototype = true;
                    fields.insert("m".to_string(), sig);
                }
            }
            let lib_ast =
                submilli_engine::desugar(submilli_engine::capture(lib_ast).unwrap(), FileId(0))
                    .unwrap();
            let (lib_sources, _) = Sources::single("lib.ts", lib_source).unwrap();
            let lib_gen = submilli_engine::codegen::codegen_package_with_type_info(
                &lib_sources,
                FileId(0),
                &lib_ast,
                &runtime_deps,
            )
            .unwrap();

            // Compile `app` against `@test/proto`.
            let deps_slice = [&lib_decl];
            let mut request = submilli_engine::front_end::script_request(&stdlib_refs);
            request.dependencies = &deps_slice;
            let checked = EngineFrontEnd
                .parse_script(app_source.to_string(), FileId(0))
                .check(&request)
                .unwrap();
            let mut app_ast = checked.typed_ast;
            for decl in &mut app_ast.types {
                if let submilli_engine::TypedTypeDecl::Class(class) = decl {
                    for field in &mut class.fields {
                        if let Some(real) = field.name.name.strip_prefix("__shadow_") {
                            field.name.name = real.to_string();
                            field.over_prototype = true;
                        }
                    }
                }
            }
            for id in app_ast.stmt_ids().unwrap() {
                let stmt = app_ast.try_stmt(id).unwrap();
                let TypedStmtKind::Expr(expr) = stmt.kind else {
                    continue;
                };
                let TypedExprKind::Call { mangled, args, .. } =
                    &app_ast.try_expr(expr).unwrap().kind
                else {
                    continue;
                };
                if mangled.as_str() == "main#writeM" {
                    app_ast.try_stmt_mut(id).unwrap().kind = TypedStmtKind::AssignField {
                        receiver: args[0],
                        value: args[1],
                        name: Ident {
                            name: "m".into(),
                            span: stmt.span,
                        },
                    };
                }
            }
            let app_ast =
                submilli_engine::desugar(submilli_engine::capture(app_ast).unwrap(), FileId(0))
                    .unwrap();
            let mut app_deps = runtime_deps;
            app_deps.push(&lib_decl);
            let app_gen = submilli_engine::codegen::codegen_with_type_info(
                app_source,
                "app.ts",
                FileId(0),
                &app_ast,
                &app_deps,
            )
            .unwrap();

            let config = RuntimeConfig::default();
            let engine = config.engine_async().unwrap();
            let data = StoreData::with_vfs("cross-pkg-proto".into(), Vfs::none());
            let mut store = config.store_async(&engine, data).unwrap();
            install_tenant_limits(&mut store);
            let mut linker = Linker::new(&engine);
            let lib_mod = Module::new(&engine, &lib_gen.wasm).unwrap();
            let app_mod = Module::new(&engine, &app_gen.wasm).unwrap();
            let linked = [LinkedPackageModule {
                module: &lib_mod,
                declaration: &lib_decl,
                type_info: &lib_gen.type_info,
            }];
            pollster::block_on(async {
                install_runtime_async(&mut linker, &mut store)
                    .await
                    .unwrap();
                install_package_modules_async(&mut linker, &mut store, &linked)
                    .await
                    .unwrap();
                store
                    .data_mut()
                    .install_type_info(app_gen.type_info.clone());
                let instance = linker
                    .instantiate_async(&mut store, &app_mod)
                    .await
                    .unwrap();
                dispatch_main_async(&mut store, &instance)
                    .await
                    .unwrap()
                    .unwrap()
            })
        })
        .unwrap()
        .join()
        .unwrap();

    assert_eq!(
        result,
        "1|20|2|a,before,m,tail|1|20|a,before,m,tail,gc|1|50|a,before,m|990|10|true|false"
    );
}

#[test]
fn own_over_prototype_edge_cases() {
    let source = r#"
class Base {
  baseVal: number = 7;
  before: number = 0;

  constructor() {
    this.before = this.m();
  }

  m(): number {
    return 1;
  }

  add(n: number): number {
    return n + 10;
  }

  boxed(): { unseenEdgeField: number } {
    return { unseenEdgeField: this.m() };
  }

  toString(): string {
    return "base-str";
  }
}

class ParamSub extends Base {
  constructor(public __shadow_m: () => number) {
    super();
  }
}

class ToStringSub extends Base {
  __shadow_toString: () => string = (): string => "own-str";
}

class AccBase {
  held: number = 10;

  get x(): number {
    return this.held;
  }

  set x(v: number) {
    this.held = v;
  }
}

class UninitAccSub extends AccBase {
  __shadow_x?: number;
}

class AltGet extends AccBase {
  get x(): number {
    return 20;
  }
}

class AltSet extends AltGet {
  last: number = 0;
  set x(v: number) {
    this.last = v;
  }
}

class AltLeaf extends AltSet {}

function callGeneric<T extends Base>(v: T): number {
  return v.boxed().unseenEdgeField;
}

function callOptional(v: Base | null): number | undefined {
  return v?.boxed().unseenEdgeField;
}

function catchTypeError(fn: () => void): string {
  try {
    fn();
    return "ok";
  } catch (e) {
    return e instanceof TypeError ? "TypeError" : "other";
  }
}

function writeM(_o: Base, _f: () => number): void {}
function deleteM(_o: Base): boolean {
  return false;
}

export function main(): string {
  // 1. Parameter property with over_prototype: true
  const ps = new ParamSub(() => 42);
  const psBase: Base = ps;
  const paramRes = [String(ps.before), String(psBase.m()), String(callGeneric(ps))];

  // 2. Generic/optional receiver + Base.toString shadowed by over_prototype field
  const ts = new ToStringSub();
  const toStrRes = [
    ts.toString(),
    String(ts),
    `${ts}`,
    String(callGeneric(ts)),
    String(callOptional(ps)),
  ];

  // 3. Spread of class with toString method must NOT invoke class method on plain object
  const plainSpread = { ...(new Base() as unknown as Record<string, unknown>) };
  const spreadToString = String(plainSpread);

  // 4. Shadowed method slots do not break structural equality or symmetry on spread objects
  const f = (): number => 99;
  const o1 = { ...(new Base() as unknown as Record<string, unknown>) };
  o1["m"] = f;
  const o2 = { ...(new Base() as unknown as Record<string, unknown>) };
  o2["m"] = f;
  const bSym = new Base();
  const symLeft: Record<string, unknown> = {
    baseVal: 7,
    before: 1,
    m: (bSym as unknown as Record<string, unknown>)["m"],
  };
  const symRight = { ...(bSym as unknown as Record<string, unknown>), z: 99 };
  const spreadEq = [
    String(o1 === o2),
    String(symLeft === symRight),
    String(symRight === symLeft),
  ].join("|");

  // 5. Dynamic index write, Object.assign, delete + Object.assign non-resurrection,
  //    re-assign, non-arrow `this` function, callee-before-arg order, Error#toJson,
  //    two-operand spread precedence, and non-callable overrides
  const b = new Base();
  const rec = b as unknown as Record<string, unknown>;
  rec["m"] = () => 88;
  const afterDyn = b.m();
  Object.assign(b, { m: () => 77 });
  const afterAssign = b.m();
  deleteM(b);
  Object.assign(b, new Base());
  const assignAfterDeleteHasOwn = Object.hasOwn(b, "m");
  const afterDel = b.m();
  writeM(b, () => 66);
  const afterReassign = b.m();
  rec["m"] = function (this: Base): number {
    return this.baseVal + 100;
  };
  const thisMethodVal = b.m();
  let protoArgCount = 0;
  const protoToOwnVal = b.add(
    (() => {
      protoArgCount += 1;
      rec["add"] = (n: number): number => n + 2000;
      return 5;
    })(),
  );
  const afterProtoToOwnVal = b.add(5);
  let ownArgCount = 0;
  rec["add"] = (n: number): number => n + 1000;
  const ownToOwnVal = b.add(
    (() => {
      ownArgCount += 1;
      rec["add"] = (n: number): number => n + 3000;
      return 5;
    })(),
  );
  const afterOwnToOwnVal = b.add(5);
  const errCustom = new Error("boom");
  (errCustom as unknown as Record<string, unknown>)["toJson"] = () => '"custom-err-json"';
  const errToJson = JSON.stringify(errCustom);
  const spreadBaseSub = {
    ...(new Base() as unknown as Record<string, unknown>),
    ...(ps as unknown as Record<string, unknown>),
  };
  const spreadSubBase = {
    ...(ps as unknown as Record<string, unknown>),
    ...(new Base() as unknown as Record<string, unknown>),
  };
  const spreadPrec = [
    String((spreadBaseSub["m"] as () => number)()),
    String((spreadSubBase["m"] as () => number)()),
  ].join(",");
  rec["m"] = 123;
  const callNonCallable = catchTypeError(() => {
    b.m();
  });
  rec["toString"] = "not-a-fn";
  const nonFnToString = catchTypeError(() => {
    String(b);
  });

  // 6. Uninitialized over_prototype field reads as undefined and rejects number methods cleanly
  const u: AccBase = new UninitAccSub();
  const uninitReadIsUndef = String((u.x as unknown) === undefined);
  const uninitReadErr = catchTypeError(() => {
    const _s: string = u.x.toFixed(0);
    if (_s === "never") {
      throw new Error("unreachable");
    }
  });

  // 7. 3-level alternating accessor halves + postfix on getter-only via interface + interface cast
  const leaf = new AltLeaf();
  const leafBase: AccBase = leaf;
  leafBase.x = 55;
  const altGetIface: { x: number } = new AltGet() as unknown as { x: number };
  const postfixErr = catchTypeError(() => {
    altGetIface.x++;
  });
  const castErr = catchTypeError(() => {
    const _v = leaf as unknown as { x: number };
    if (_v.x === 999) {
      throw new Error("unreachable");
    }
  });

  // 8. Integer-key boundary ordering (2^32-1, "01", "-1" are non-indices; overwrite keeps position)
  const boundary: Record<string, number> = {};
  boundary["4294967295"] = 1;
  boundary["01"] = 2;
  boundary["-1"] = 3;
  boundary["10"] = 4;
  boundary["2"] = 5;
  boundary["0"] = 6;
  const boundarySpread = { ...boundary, ["01"]: 99 };
  const boundaryKeys = [
    Object.keys(boundary).join(","),
    Object.keys(boundarySpread).join(","),
  ];

  return [
    paramRes.join("|"),
    toStrRes.join("|"),
    spreadToString,
    spreadEq,
    [
      String(afterDyn),
      String(afterAssign),
      String(assignAfterDeleteHasOwn),
      String(afterDel),
      String(afterReassign),
      String(thisMethodVal),
      `${protoArgCount}:${protoToOwnVal}:${afterProtoToOwnVal}/${ownArgCount}:${ownToOwnVal}:${afterOwnToOwnVal}`,
      errToJson,
      spreadPrec,
      callNonCallable,
      nonFnToString,
    ].join("|"),
    [uninitReadIsUndef, uninitReadErr].join("|"),
    [String(leaf.last), String((leafBase.x as unknown) === undefined), postfixErr, castErr].join(
      "|",
    ),
    boundaryKeys.join("|"),
  ].join(" // ");
}
"#;
    assert_eq!(
        run(source, |ast| {
            let mut marked = 0;
            for decl in &mut ast.types {
                let submilli_engine::TypedTypeDecl::Class(class) = decl else {
                    continue;
                };
                for field in &mut class.fields {
                    if let Some(real) = field.name.name.strip_prefix("__shadow_") {
                        field.name.name = real.to_string();
                        field.over_prototype = true;
                        marked += 1;
                    }
                }
                if let Some(ctor) = &mut class.constructor {
                    for param in &mut ctor.params {
                        if let Some(real) = param.name.name.strip_prefix("__shadow_") {
                            param.name.name = real.to_string();
                        }
                    }
                }
            }
            assert_eq!(marked, 3);
            for id in ast.stmt_ids().unwrap() {
                let stmt = ast.try_stmt(id).unwrap();
                let TypedStmtKind::Expr(expr) = stmt.kind else {
                    continue;
                };
                let TypedExprKind::Call { mangled, args, .. } = &ast.try_expr(expr).unwrap().kind
                else {
                    continue;
                };
                if mangled.as_str() == "main#writeM" {
                    ast.try_stmt_mut(id).unwrap().kind = TypedStmtKind::AssignField {
                        receiver: args[0],
                        value: args[1],
                        name: Ident {
                            name: "m".into(),
                            span: stmt.span,
                        },
                    };
                }
            }
            for id in ast.expr_ids().unwrap() {
                let expr = ast.try_expr(id).unwrap().clone();
                let TypedExprKind::Call { mangled, args, .. } = expr.kind else {
                    continue;
                };
                if mangled.as_str() == "main#deleteM" {
                    ast.try_expr_mut(id).unwrap().kind = TypedExprKind::DeleteField {
                        receiver: args[0],
                        name: Ident {
                            name: "m".into(),
                            span: expr.span,
                        },
                    };
                }
            }
        }),
        [
            "1|42|42",
            "own-str|own-str|own-str|1|42",
            "[object Object]",
            "true|false|false",
            r#"88|77|false|1|66|107|1:15:2005/1:1005:3005|"custom-err-json"|42,42|TypeError|TypeError"#,
            "true|TypeError",
            "55|true|TypeError|TypeError",
            "0,2,10,4294967295,01,-1|0,2,10,4294967295,01,-1",
        ]
        .join(" // ")
    );
}
