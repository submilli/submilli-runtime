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
