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

#[test]
fn abstract_property_defines_no_storage_until_subclass_defines_it() {
    let source = r#"
        class Shape {
            x: number = 0;
            inBase: boolean;
            ownInBase: boolean;
            keysInBase: string;
            spreadInBase: string;
            jsonInBase: string;
            constructor() {
                this.inBase = "x" in this;
                this.ownInBase = Object.hasOwn(this, "x");
                this.keysInBase = Object.keys(this).join(",");
                const view: { x?: number; inBase: boolean } = this;
                this.spreadInBase = Object.keys({ ...view }).join(",");
                this.jsonInBase = JSON.stringify(this);
            }
            hasX(): boolean { return "x" in this; }
            readX(): number { return this.x; }
            writeX(v: number): void { this.x = v; }
            bumpX(): number { return this.x++; }
        }

        class Pt extends Shape {
            y: number = 2;
            x: number = 1;
        }

        class GrandPt extends Pt {}

        class ParamPt extends Shape {
            constructor(public x: number) {
                super();
            }
        }

        class InitInCtor extends Shape {
            y: number = 2;
            beforeWrite: boolean;
            constructor(v: number) {
                super();
                this.beforeWrite = "x" in this;
                this.x = v;
            }
        }

        class Mid extends Shape {
            x: number = 0;
        }

        class Leaf extends Mid {
            x: number = 99;
        }

        function readOptional(s: Shape | undefined): number | undefined {
            return s?.x;
        }

        class ShapeZ {
            z: number = 0;
            readZ(): number { return this.z; }
        }

        class PtZ extends ShapeZ {
            y: number = 2;
            z: number = 1;
        }

        class ShapeMaybe {
            u: number | undefined = undefined;
            inBase: boolean;
            constructor() {
                this.inBase = "u" in this;
            }
        }

        class UninitSub extends ShapeMaybe {
            u: number | undefined;
        }

        function main(): string {
            const pt = new Pt();
            const sPt: Shape = pt;
            assert(!pt.inBase, "Pt: 'x' in this is false in base ctor");
            assert(!pt.ownInBase, "Pt: hasOwn is false in base ctor");
            assert(!pt.keysInBase.includes("x"), "Pt: Object.keys omits x in base ctor");
            assert(!pt.spreadInBase.includes("x"), "Pt: spread omits x in base ctor");
            assert(!pt.jsonInBase.includes('"x"'), "Pt: JSON omits x in base ctor");
            assert(sPt.hasX() && Object.hasOwn(pt, "x"), "Pt: x present after subclass init");
            assert(sPt.readX() === 1 && pt.x === 1, "Pt: read through base and subclass");
            sPt.writeX(10);
            assert(pt.x === 10 && sPt.readX() === 10, "Pt: write through base");
            assert(sPt.bumpX() === 10 && pt.x === 11, "Pt: postfix through base");
            assert(readOptional(sPt) === 11 && readOptional(undefined) === undefined, "Pt: optional chain");
            assert(Object.keys(pt).includes("x"), "Pt: Object.keys includes x");
            const ptView: { x: number; y: number } = pt;
            assert(Object.keys({ ...ptView }).includes("x"), "Pt: spread includes x");
            assert(JSON.stringify(pt).includes('"x":11'), "Pt: JSON includes x");

            const gp = new GrandPt();
            assert(!gp.inBase && gp.readX() === 1 && Object.hasOwn(gp, "x"), "GrandPt: absent in base ctor, present after");

            const pp = new ParamPt(7);
            assert(!pp.inBase && pp.readX() === 7 && Object.hasOwn(pp, "x"), "ParamPt");

            const ic = new InitInCtor(42);
            const sIc: Shape = ic;
            assert(!ic.inBase && !ic.beforeWrite, "InitInCtor: absent until constructor write");
            assert(sIc.hasX() && Object.hasOwn(ic, "x"), "InitInCtor: present after constructor write");
            assert(sIc.readX() === 42 && ic.x === 42, "InitInCtor: read value");
            const icKeys = Object.keys(ic);
            assert(icKeys.indexOf("y") >= 0 && icKeys.indexOf("x") > icKeys.indexOf("y"), "InitInCtor: x inserted after y");
            assert(new InitInCtor(1) === new InitInCtor(1), "InitInCtor: structural equality matches equal x");
            assert(new InitInCtor(1) !== new InitInCtor(2), "InitInCtor: structural equality distinguishes different x");
            assert((new InitInCtor(1) as unknown) !== ({ y: 2, x: 1 } as unknown), "InitInCtor: class != plain object");
            const icMap = new Map<InitInCtor, string>();
            icMap.set(new InitInCtor(1), "one");
            icMap.set(new InitInCtor(2), "two");
            assert(icMap.size === 2 && icMap.get(new InitInCtor(1)) === "one" && icMap.get(new InitInCtor(2)) === "two", "InitInCtor: Map hash+equals");
            sIc.writeX(50);
            assert(sIc.bumpX() === 50 && sIc.readX() === 51 && readOptional(sIc) === 51, "InitInCtor: write/bump/chain");

            const leaf = new Leaf();
            assert(!leaf.inBase && leaf.readX() === 99, "Mid->Leaf multi-level abstract");

            const ptz = new PtZ();
            assert(Object.keys(ptz).join(",") === "y,z", "PtZ: z not pinned into base prefix");
            assert(ptz.readZ() === 1, "PtZ: readZ");

            const us = new UninitSub();
            assert(!us.inBase, "UninitSub: absent in base ctor");
            assert("u" in us && Object.hasOwn(us, "u") && Object.keys(us).includes("u"), "UninitSub: defined at subclass field init");
            assert(us.u === undefined, "UninitSub: value is undefined");

            return "ok";
        }
    "#;
    assert_eq!(
        run(source, |ast| {
            mark_class_field_abstract(ast, "Shape", "x");
            mark_class_field_abstract(ast, "Mid", "x");
            mark_class_field_abstract(ast, "ShapeZ", "z");
            mark_class_field_abstract(ast, "ShapeMaybe", "u");
        }),
        "ok"
    );
}

#[test]
fn abstract_property_implemented_by_subclass_accessor() {
    let source = r#"
        class Shape {
            x: number = 0;
            inBase: boolean;
            ownInBase: boolean;
            keysInBase: string;
            constructor() {
                this.inBase = "x" in this;
                this.ownInBase = Object.hasOwn(this, "x");
                this.keysInBase = Object.keys(this).join(",");
            }
            readX(): number { return this.x; }
            writeX(v: number): void { this.x = v; }
            bumpX(): number { return this.x++; }
        }

        class Computed extends Shape {
            private v: number = 10;
            get _x(): number { return this.v; }
            set _x(n: number) { this.v = n; }
        }

        class ReadOnlyComputed extends Shape {
            get _x(): number { return 77; }
        }

        class WriteOnlyComputed extends Shape {
            last: number = 0;
            set _x(n: number) { this.last = n; }
        }

        function readOptional(s: Shape | undefined): number | undefined {
            return s?.x;
        }

        function writeGeneric<T extends Shape>(s: T, v: number): void {
            s.x = v;
        }

        function readGenericOpt<T extends Shape>(s: T | undefined): number | undefined {
            return s?.x;
        }

        function readReadonly<T extends Shape>(target: Readonly<T>): number {
            return target.x;
        }

        function readNested<T extends { s: Shape }>(holder: T): number {
            return holder.s.x;
        }

        function main(): string {
            const c = new Computed();
            const s: Shape = c;
            assert(c.inBase, "Computed: prototype getter makes 'x' in this true in base ctor");
            assert(!c.ownInBase, "Computed: hasOwn is false in base ctor");
            assert(!c.keysInBase.includes("x"), "Computed: Object.keys omits x in base ctor");
            assert("x" in c && !Object.hasOwn(c, "x"), "Computed: prototype accessor, not own property");
            assert(!Object.keys(c).includes("x"), "Computed: Object.keys omits x");
            const cView: { x?: number; inBase: boolean } = c;
            assert(!Object.keys({ ...cView }).includes("x"), "Computed: spread omits x");
            assert(s.readX() === 10 && readOptional(s) === 10, "Computed: getter via base");
            s.writeX(25);
            assert(s.readX() === 25 && !Object.hasOwn(c, "x"), "Computed: setter via base keeps no own property");
            assert(s.bumpX() === 25 && s.readX() === 26, "Computed: postfix via base uses getter+setter");
            writeGeneric(c, 40);
            assert(readGenericOpt(c) === 40 && !Object.hasOwn(c, "x"), "Computed: generic receiver write+read");

            const ro = new ReadOnlyComputed();
            const sRo: Shape = ro;
            assert(sRo.readX() === 77 && !Object.hasOwn(ro, "x") && !Object.keys(ro).includes("x"), "ReadOnlyComputed: getter works");
            let threw = false;
            try {
                sRo.writeX(88);
            } catch (e) {
                threw = e instanceof TypeError;
            }
            assert(threw, "ReadOnlyComputed: write through base throws TypeError");
            let genericThrew = false;
            try {
                writeGeneric(ro, 88);
            } catch (e) {
                genericThrew = e instanceof TypeError;
            }
            assert(genericThrew, "ReadOnlyComputed: write through generic receiver throws TypeError");

            const wo = new WriteOnlyComputed();
            const sWo: Shape = wo;
            assert(wo.inBase && !Object.hasOwn(wo, "x") && !Object.keys(wo).includes("x"), "WriteOnlyComputed: prototype setter");
            sWo.writeX(91);
            assert(wo.last === 91 && !Object.hasOwn(wo, "x"), "WriteOnlyComputed: setter invoked");

            // Deferred generic receiver shapes (Mapped / IndexedAccess) still invoke accessor.
            assert(readReadonly(c) === 40, "Readonly<T> invokes getter on Computed");
            assert(readNested({ s: c }) === 40, "T['s'] invokes getter on Computed");
            return "ok";
        }
    "#;
    assert_eq!(
        run(source, |ast| {
            mark_class_field_abstract(ast, "Shape", "x");
            rename_class_accessors(
                ast,
                &["Computed", "ReadOnlyComputed", "WriteOnlyComputed"],
                "_x",
                "x",
            );
        }),
        "ok"
    );
}

#[test]
fn abstract_property_works_across_package_boundary() {
    use submilli_engine::compile::{PackageSourceModule, compile_package_checked};
    use submilli_engine::runtime::install_runtime_async;
    use submilli_engine::{EngineFrontEnd, FrontEnd, ModulePath, script_request};

    let dep_source = r#"
        /** Base shape. */
        export class Shape {
            x: number = 0;
            inBase: boolean;
            constructor() {
                this.inBase = "x" in this;
            }
            readX(): number { return this.x; }
            writeX(v: number): void { this.x = v; }
        }

        /** Concrete point. */
        export class ExportedPt extends Shape {
            y: number = 2;
            x: number = 5;
        }

        /**
         * Generic writer over Shape.
         * @param s Target shape.
         * @param v New coordinate.
         */
        export function writeShapeGeneric<T extends Shape>(s: T, v: number): void {
            s.x = v;
        }

        /**
         * Generic optional reader over Shape.
         * @param s Optional shape.
         * @returns Coordinate or undefined.
         */
        export function readShapeGenericOpt<T extends Shape>(s: T | undefined): number | undefined {
            return s?.x;
        }

        /** Wide base with abstract field. */
        export class WideBase {
            v: number | string = 0;
            setV(x: number | string): void { this.v = x; }
            getV(): number | string { return this.v; }
        }

        /** Narrowed abstract intermediate class. */
        export class NarrowMid extends WideBase {
            v: number = 0;
            getNarrow(): number { return this.v; }
        }

        /** Wide optional base with abstract field. */
        export class WideOptBase {
            v?: number | string;
            setV(x: number | string | undefined): void { this.v = x; }
        }

        /** Generic narrowed abstract intermediate class. */
        export class GenMid<T extends number | string> extends WideOptBase {
            v?: T;
            readGen(): T | undefined { return this.v; }
        }

        /** Exported concrete type alias for testing AliasRef NonNull checks. */
        export type Id = string;

        /**
         * Read generic narrowed field through erased type parameter.
         * @param m Target instance.
         * @returns Field value or undefined.
         */
        export function readGenMidErased<U extends number | string>(m: GenMid<U>): U | undefined {
            return m.v;
        }
    "#;

    let (dep_wasm, dep_decl) = std::thread::Builder::new()
        .stack_size(submilli_engine::compiler_limits::COMPILER_STACK_BYTES)
        .spawn(move || {
            let compiled = compile_package_checked(
                "dep",
                ModulePath::from("lib"),
                &[PackageSourceModule {
                    path: ModulePath::from("lib"),
                    source: dep_source,
                }],
                &[],
            )
            .unwrap();
            let (mut ast, diagnostics) = submilli_engine::compile::typecheck_package_to_typed_ast(
                "dep",
                ModulePath::from("lib"),
                &[PackageSourceModule {
                    path: ModulePath::from("lib"),
                    source: dep_source,
                }],
            );
            assert!(diagnostics.is_empty(), "{diagnostics:?}");
            mark_class_field_abstract(&mut ast, "Shape", "x");
            mark_class_field_abstract(&mut ast, "WideBase", "v");
            mark_class_field_abstract(&mut ast, "NarrowMid", "v");
            mark_class_field_abstract(&mut ast, "WideOptBase", "v");
            mark_class_field_abstract(&mut ast, "GenMid", "v");
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
                dep_source,
                "lib.ts",
                FileId(0),
                &ast,
                &dependencies,
            )
            .unwrap();
            let mut decl = compiled.declaration;
            for sym in decl
                .types
                .values_mut()
                .chain(decl.runtime_types.values_mut())
            {
                if sym.name == "Shape"
                    && let submilli_engine::TypeKind::Class { fields, .. } = &mut sym.kind
                    && let Some(x) = fields.get_mut("x")
                {
                    x.abstract_property = true;
                }
                if matches!(
                    sym.name.as_str(),
                    "WideBase" | "NarrowMid" | "WideOptBase" | "GenMid"
                ) && let submilli_engine::TypeKind::Class { fields, .. } = &mut sym.kind
                    && let Some(v) = fields.get_mut("v")
                {
                    v.abstract_property = true;
                }
            }
            let json = serde_json::to_string(&decl).unwrap();
            let round_tripped: submilli_engine::PackageDeclaration =
                serde_json::from_str(&json).unwrap();
            (generated.wasm, round_tripped)
        })
        .unwrap()
        .join()
        .unwrap();

    let consumer_source = r#"
        import {
            Id,
            Shape,
            ExportedPt,
            writeShapeGeneric,
            readShapeGenericOpt,
            WideBase,
            NarrowMid,
            WideOptBase,
            GenMid,
            readGenMidErased,
        } from "dep";

        class LocalPt extends Shape {
            y: number = 3;
            x: number = 11;
        }

        class LocalGrandPt extends ExportedPt {}

        class LocalInitInCtor extends Shape {
            y: number = 4;
            beforeWrite: boolean;
            constructor() {
                super();
                this.beforeWrite = "x" in this;
                this.x = 77;
            }
        }

        class LocalComputed extends Shape {
            private v: number = 30;
            get _x(): number { return this.v; }
            set _x(n: number) { this.v = n; }
        }

        class LocalReadOnlyComputed extends Shape {
            get _x(): number { return 66; }
        }

        class NarrowLeaf extends NarrowMid {
            v: number = 1;
        }

        class LocalLazyGenLeaf extends GenMid<number> {}

        class LocalAliasBase {
            id: Id | null = null;
            setId(v: Id | null): void { this.id = v; }
        }

        class LocalAliasMid extends LocalAliasBase {
            id: Id = "";
            absentThrew: boolean = false;
            constructor() {
                super();
                try {
                    const _v: Id = this.id;
                    assert(_v.length >= 0);
                } catch (e) {
                    this.absentThrew = e instanceof TypeError;
                }
            }
            readId(): Id { return this.id; }
        }

        class LocalAliasLeaf extends LocalAliasMid {
            id: Id = "abc";
        }

        function main(): string {
            const lp = new LocalPt();
            assert(!lp.inBase && lp.readX() === 11 && Object.hasOwn(lp, "x"), "LocalPt");
            lp.writeX(12);
            assert(lp.x === 12, "LocalPt write");

            const gp = new LocalGrandPt();
            assert(!gp.inBase && gp.readX() === 5 && Object.hasOwn(gp, "x"), "LocalGrandPt");

            const ic = new LocalInitInCtor();
            assert(!ic.inBase && !ic.beforeWrite && ic.readX() === 77, "LocalInitInCtor");
            const icKeys = Object.keys(ic);
            assert(icKeys.indexOf("x") > icKeys.indexOf("y"), "LocalInitInCtor key order");
            const ic2 = new LocalInitInCtor();
            assert(ic === ic2, "LocalInitInCtor equal when dynamic abstract field matches");
            const icMap = new Map<LocalInitInCtor, string>();
            icMap.set(ic, "cross-pkg");
            assert(icMap.get(ic2) === "cross-pkg", "LocalInitInCtor hash matches");
            ic2.writeX(88);
            assert(ic !== ic2 && icMap.get(ic2) === undefined, "LocalInitInCtor unequal after dynamic mutation");

            const c = new LocalComputed();
            assert(c.inBase && !Object.hasOwn(c, "x") && c.readX() === 30, "LocalComputed get");
            c.writeX(35);
            assert(c.readX() === 35 && !Object.hasOwn(c, "x"), "LocalComputed set");
            writeShapeGeneric(c, 45);
            assert(readShapeGenericOpt(c) === 45 && !Object.hasOwn(c, "x"), "LocalComputed generic cross-pkg");

            const ro = new LocalReadOnlyComputed();
            assert(readShapeGenericOpt(ro) === 66, "LocalReadOnlyComputed generic read");
            let threw = false;
            try {
                writeShapeGeneric(ro, 99);
            } catch (e) {
                threw = e instanceof TypeError;
            }
            assert(threw, "LocalReadOnlyComputed generic cross-pkg write throws TypeError");

            const nl = new NarrowLeaf();
            const wb: WideBase = nl;
            wb.setV("bad");
            assert(wb.getV() === "bad", "NarrowLeaf: unnarrowed ancestor view returns written value");
            let narrowedThrew = false;
            try {
                nl.getNarrow();
            } catch (e) {
                narrowedThrew = e instanceof TypeError;
            }
            assert(narrowedThrew, "NarrowLeaf: narrowed abstract intermediate enforces TypeError");

            // Cross-package generic narrowed abstract property with post-construction dynamic write.
            const gl = new LocalLazyGenLeaf();
            const glWide: GenMid<number | string> = gl;
            assert(readGenMidErased(glWide) === undefined, "LocalLazyGenLeaf: absent optional ok");
            const glBase: WideOptBase = gl;
            glBase.setV("bad");
            let genCrossThrew = false;
            try {
                readGenMidErased(glWide);
            } catch (e) {
                genCrossThrew = e instanceof TypeError;
            }
            assert(genCrossThrew, "LocalLazyGenLeaf: cross-package erased read enforces concrete guard");

            // Imported concrete `Type::AliasRef` (`Id | null` -> `Id`) must not bypass `NonNull`.
            const al = new LocalAliasLeaf();
            assert(al.absentThrew && al.readId() === "abc", "LocalAliasLeaf: AliasRef NonNull rejects absent undefined");
            const alBase: LocalAliasBase = al;
            alBase.setId(null);
            let aliasNullThrew = false;
            try {
                al.readId();
            } catch (e) {
                aliasNullThrew = e instanceof TypeError;
            }
            assert(aliasNullThrew, "LocalAliasLeaf: AliasRef NonNull rejects null");

            return "ok";
        }
    "#;

    let consumer_wasm = std::thread::Builder::new()
        .stack_size(submilli_engine::compiler_limits::COMPILER_STACK_BYTES)
        .spawn(move || {
            let stdlib = [&dep_decl];
            let request = script_request(&stdlib);
            let checked = EngineFrontEnd
                .parse_script(consumer_source.to_string(), FileId(0))
                .check(&request)
                .unwrap();
            assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
            let mut ast = checked.typed_ast;
            mark_class_field_abstract(&mut ast, "LocalAliasBase", "id");
            mark_class_field_abstract(&mut ast, "LocalAliasMid", "id");
            // Retain bare `Type::AliasRef` on `LocalAliasMid`'s `id` field/reads
            // to model external declarations that preserve unexpanded alias refs.
            let to_alias_ref = |ty: &Type| match ty {
                Type::Alias {
                    mangled,
                    package,
                    name,
                    args,
                    ..
                } => Some(Type::AliasRef {
                    mangled: mangled.clone(),
                    package: package.clone(),
                    name: name.clone(),
                    args: args.clone(),
                }),
                _ => None,
            };
            let mut rewrote_reads = 0usize;
            for id in ast.expr_ids().unwrap() {
                if let TypedExprKind::FieldAccess { name, .. } = &ast.try_expr(id).unwrap().kind
                    && name.name == "id"
                    && let Some(alias_ref) = to_alias_ref(&ast.try_expr(id).unwrap().ty)
                {
                    ast.try_expr_mut(id).unwrap().ty = alias_ref;
                    rewrote_reads += 1;
                }
            }
            let mut rewrote_target = false;
            for decl in &mut ast.types {
                if let submilli_engine::TypedTypeDecl::Class(class) = decl
                    && class.name.name == "LocalAliasMid"
                {
                    for field in &mut class.fields {
                        if field.name.name == "id"
                            && let Some(check) = &mut field.narrowing_check
                            && let Some(target) = check.minimal_test_target.as_ref()
                            && let Some(alias_ref) = to_alias_ref(target)
                        {
                            check.minimal_test_target = Some(alias_ref);
                            rewrote_target = true;
                        }
                    }
                }
            }
            assert!(
                rewrote_reads > 0 && rewrote_target,
                "expected LocalAliasMid `id` reads and minimal_test_target to rewrite to AliasRef"
            );
            rename_class_accessors(
                &mut ast,
                &["LocalComputed", "LocalReadOnlyComputed"],
                "_x",
                "x",
            );
            let ast = submilli_engine::desugar(submilli_engine::capture(ast).unwrap(), FileId(0))
                .unwrap();
            let (prelude, host, internal) =
                submilli_engine::runtime::prelude::cached_runtime_package_declarations();
            let mut dependencies = prelude
                .iter()
                .chain(host)
                .chain(internal)
                .collect::<Vec<_>>();
            dependencies.push(&dep_decl);
            submilli_engine::codegen::codegen_with_type_info(
                consumer_source,
                "consumer.ts",
                FileId(0),
                &ast,
                &dependencies,
            )
            .unwrap()
            .wasm
        })
        .unwrap()
        .join()
        .unwrap();

    let value = pollster::block_on(async move {
        let config = RuntimeConfig::default();
        let engine = config.engine().unwrap();
        let mut store = config
            .store_async(
                &engine,
                submilli_engine::runtime::StoreData::with_vfs(
                    "abstract-prop-pkg".into(),
                    submilli_engine::runtime::Vfs::none(),
                ),
            )
            .unwrap();
        let mut linker = wasmtime::Linker::new(&engine);
        install_runtime_async(&mut linker, &mut store)
            .await
            .unwrap();
        let dep_module = wasmtime::Module::new(&engine, &dep_wasm).unwrap();
        let dep_instance = linker
            .instantiate_async(&mut store, &dep_module)
            .await
            .unwrap();
        linker.instance(&mut store, "dep", dep_instance).unwrap();
        let consumer_module = wasmtime::Module::new(&engine, &consumer_wasm).unwrap();
        let consumer_instance = linker
            .instantiate_async(&mut store, &consumer_module)
            .await
            .unwrap();
        submilli_engine::dispatch_main_async(&mut store, &consumer_instance)
            .await
            .unwrap()
            .unwrap()
    });
    assert_eq!(value, "ok");
}

#[test]
fn abstract_property_narrowing_and_base_ctor_writes() {
    let source = r#"
        class Root {
            value: unknown = 0;
            setValue(v: unknown): void { this.value = v; }
            readValue(): unknown { return this.value; }
        }

        class Child extends Root {
            value: { n: number } = { n: 1 };
        }

        class Base {
            x: number | string = 0;
            setX(v: number | string): void { this.x = v; }
            readBase(): number | string { return this.x; }
        }

        class Mid<T extends number | string> extends Base {
            x: T;
            constructor(init: T) {
                super();
                this.x = init;
            }
            readMid(): T { return this.x; }
        }

        class Leaf extends Mid<number> {
            x: number = 1;
            constructor() { super(1); }
        }

        class InitGenericLeaf extends Mid<number> {
            constructor() { super(1); }
        }

        class CtorReadLeaf extends Mid<number> {
            ctorErasedThrew: boolean = false;
            constructor() {
                super(1);
                const asBase: Base = this;
                asBase.setX("during-ctor");
                const asWide: Mid<number | string> = this;
                try {
                    readMidErased(asWide);
                } catch (e) {
                    this.ctorErasedThrew = e instanceof TypeError;
                }
                this.x = 2;
            }
        }

        class AccessorGenericLeaf extends Mid<number> {
            private stored: number = 7;
            constructor() { super(7); }
            get _x(): number { return this.stored; }
            set _x(v: number) { this.stored = v; }
        }

        class BaseMaybe {
            x: number | string | undefined = undefined;
            setX(v: number | string | undefined): void { this.x = v; }
        }

        class MidOpt<T extends number | string> extends BaseMaybe {
            x?: T;
        }

        class LazyOptLeaf extends MidOpt<number> {}

        class MidStrict<T extends number | string | undefined> extends BaseMaybe {
            x: T;
            constructor(dummy: T) {
                super();
                this.x = dummy;
            }
        }

        class LazyStrictLeaf extends MidStrict<number> {
            constructor() { super(0); }
        }

        function readMidOptErased<U extends number | string>(m: MidOpt<U>): U | undefined {
            return m.x;
        }

        function readMidStrictErased<U extends number | string | undefined>(m: MidStrict<U>): U {
            return m.x;
        }

        function peekStructural<T>(v: { x?: T }): T | undefined {
            return v.x;
        }

        function peekNumber(v: { x?: number }): number | undefined {
            return v.x;
        }

        class PeekBase {
            x: number = 0;
            early: number | undefined;
            earlyNum: number | undefined;
            constructor() {
                this.early = peekStructural<number>(this);
                this.earlyNum = peekNumber(this);
            }
        }

        class NarrowLitMid extends PeekBase {
            x: 1 | 2 = 1;
        }

        class InitLitSub extends NarrowLitMid {
            constructor() {
                super();
                this.x = 1;
            }
        }

        class ConcreteLitSub extends NarrowLitMid {
            x: 1 | 2 = 1;
        }

        class LegacyNoDeclMid extends Base {
            x: number = 0;
            readLegacy(): number { return this.x; }
        }

        class LegacyNoDeclSub extends LegacyNoDeclMid {
            constructor() {
                super();
                this.x = 9;
            }
        }

        class MultiBase {
            a: number | string | boolean = 0;
            b: number | string = 0;
            c: number | string = 0;
            setAll(a: number | string | boolean, b: number | string, c: number | string): void {
                this.a = a;
                this.b = b;
                this.c = c;
            }
        }

        class MultiMid<T extends number | string> extends MultiBase {
            a: T;
            b: T;
            c: T;
            constructor(init: T) {
                super();
                this.a = init;
                this.b = init;
                this.c = init;
            }
            readA(): T { return this.a; }
            readB(): T { return this.b; }
            readC(): T { return this.c; }
        }

        class MultiSub<U extends number> extends MultiMid<number> {
            a: U;
            constructor(init: U) {
                super(init);
                this.a = init;
            }
            readSubA(): U { return this.a; }
        }

        class MultiLeaf extends MultiSub<1 | 2> {
            constructor() { super(1); }
        }

        function readMultiMidA<X extends number | string>(m: MultiMid<X>): X { return m.a; }
        function readMultiMidB<X extends number | string>(m: MultiMid<X>): X { return m.b; }
        function readMultiMidC<X extends number | string>(m: MultiMid<X>): X { return m.c; }
        function readMultiSubA<X extends number>(m: MultiSub<X>): X { return m.a; }

        function readMidKey<U extends number | string>(m: { x: U }, k: "x"): U { return m[k]; }
        function readReadonlyMid<T extends Mid<number>>(r: Readonly<T>): number { return r.x; }
        function deleteField(o: unknown, key: string): boolean { return true; }

        class OptMethodBase {
            f?(): number;
        }

        class OptMethodSub extends OptMethodBase {
            f(): number { return 42; }
        }

        class NarrowFBase {
            f: unknown = 0;
        }

        class NarrowFSub extends NarrowFBase {
            f: number = 1;
        }

        class DeleteOptBase {
            x?: number;
        }

        class DeleteConcreteOptSub extends DeleteOptBase {
            y: number = 1;
            x?: number = 7;
        }

        class DeleteDynamicOptSub extends DeleteOptBase {
            y: number = 1;
            constructor() {
                super();
                this.x = 7;
            }
        }

        class NarrowMid extends Base {
            x: number = 0;
            absentThrew: boolean = false;
            constructor() {
                super();
                try {
                    const _early = this.readNarrow();
                    assert(_early === -1);
                } catch (e) {
                    this.absentThrew = e instanceof TypeError;
                }
            }
            readNarrow(): number { return this.x; }
        }

        class ConcreteNarrowOverAbstract extends NarrowMid {
            x: 1 | 2 = 1;
        }

        class InitLeaf extends NarrowMid {
            constructor() {
                super();
                this.x = 7;
            }
        }

        class WideNum {
            x: number = 0;
            bump(): number { return this.x++; }
        }

        class NarrowNum extends WideNum {
            x: 1 | 2 = 1;
        }

        class BaseWrites {
            opt: number | string | undefined = undefined;
            constructor() {
                this.opt = "from-base";
            }
        }

        class OptResetSub extends BaseWrites {
            opt?: number;
        }

        type RootAlias = Root;

        function readMidErased<U extends number | string>(m: Mid<U>): U {
            return m.x;
        }

        function readMidOpt<U extends number | string>(m: Mid<U> | undefined): U | undefined {
            return m?.x;
        }

        function main(): string {
            // Unnarrowed ancestor view returns written value without enforcing descendant narrowing.
            const child = new Child();
            const asRoot: Root = child;
            const asAlias: RootAlias = child;
            const asReadonly: Readonly<Root> = child;
            asRoot.setValue(42);
            assert(asRoot.readValue() === 42 && asAlias.value === 42 && asReadonly.value === 42, "ancestor view skips child guard");
            let childThrew = false;
            try {
                const _bad = child.value;
                assert(_bad.n === -1);
            } catch (e) {
                childThrew = e instanceof TypeError;
            }
            assert(childThrew, "child view enforces narrowing guard");

            // Generic abstract narrowing guards both concrete subclass slots and dynamic constructor-written slots.
            const leaf = new Leaf();
            const bLeaf: Base = leaf;
            bLeaf.setX("oops");
            assert(bLeaf.readBase() === "oops", "generic base view reads wide value");
            let genericThrew = false;
            try {
                const _bad = leaf.x;
                assert(_bad === -1);
            } catch (e) {
                genericThrew = e instanceof TypeError;
            }
            assert(genericThrew, "generic narrowed leaf enforces guard on concrete slot");

            let readMidThrew = false;
            try {
                const _bad: number = leaf.readMid();
                assert(_bad === -1);
            } catch (e) {
                readMidThrew = e instanceof TypeError;
            }
            assert(readMidThrew, "Mid::readMid enforces generic guard");

            const wideMidLeaf: Mid<number | string> = leaf;
            let erasedThrew = false;
            try {
                readMidErased(wideMidLeaf);
            } catch (e) {
                erasedThrew = e instanceof TypeError;
            }
            assert(erasedThrew, "erased Mid<number | string> read enforces concrete instance guard");

            let optThrew = false;
            try {
                readMidOpt(wideMidLeaf);
            } catch (e) {
                optThrew = e instanceof TypeError;
            }
            assert(optThrew, "erased Mid<number | string> optional chain enforces concrete instance guard");

            const initLeaf = new InitGenericLeaf();
            assert(initLeaf.readMid() === 1 && readMidErased(initLeaf) === 1, "InitGenericLeaf initial read");
            const bInitLeaf: Base = initLeaf;
            bInitLeaf.setX("oops");
            assert(bInitLeaf.readBase() === "oops", "InitGenericLeaf base view reads wide value");
            let initReadMidThrew = false;
            try {
                const _bad: number = initLeaf.readMid();
                assert(_bad === -1);
            } catch (e) {
                initReadMidThrew = e instanceof TypeError;
            }
            assert(initReadMidThrew, "InitGenericLeaf readMid enforces generic guard");

            const wideInitLeaf: Mid<number | string> = initLeaf;
            let initErasedThrew = false;
            try {
                readMidErased(wideInitLeaf);
            } catch (e) {
                initErasedThrew = e instanceof TypeError;
            }
            assert(initErasedThrew, "InitGenericLeaf erased read enforces dynamic instance guard");

            let initOptThrew = false;
            try {
                readMidOpt(wideInitLeaf);
            } catch (e) {
                initOptThrew = e instanceof TypeError;
            }
            assert(initOptThrew, "InitGenericLeaf erased optional chain enforces dynamic instance guard");

            // Erased read during construction still sees guard installed by constructor_argument.
            const ctorRead = new CtorReadLeaf();
            assert(ctorRead.ctorErasedThrew, "CtorReadLeaf: erased read during ctor throws TypeError");

            // Accessor-backed subclass of generic narrowed Mid<T>.
            const accLeaf = new AccessorGenericLeaf();
            const wideAcc: Mid<number | string> = accLeaf;
            assert(accLeaf.readMid() === 7 && readMidErased(wideAcc) === 7, "AccessorGenericLeaf: getter works");

            // Post-construction dynamic write on optional generic narrowed abstract property.
            const lazyOpt = new LazyOptLeaf();
            const wideLazyOpt: MidOpt<number | string> = lazyOpt;
            assert(readMidOptErased(wideLazyOpt) === undefined, "LazyOptLeaf: absent optional returns undefined");
            const lazyOptBase: BaseMaybe = lazyOpt;
            lazyOptBase.setX("late-oops");
            let lazyOptThrew = false;
            try {
                readMidOptErased(wideLazyOpt);
            } catch (e) {
                lazyOptThrew = e instanceof TypeError;
            }
            assert(lazyOptThrew, "LazyOptLeaf: post-construction dynamic write enforced on erased read");

            // Absent read on non-optional generic narrowed abstract property rejects undefined via guard.
            const lazyStrict = new LazyStrictLeaf();
            const wideLazyStrict: MidStrict<number | string | undefined> = lazyStrict;
            let lazyStrictThrew = false;
            try {
                readMidStrictErased(wideLazyStrict);
            } catch (e) {
                lazyStrictThrew = e instanceof TypeError;
            }
            assert(lazyStrictThrew, "LazyStrictLeaf: absent read rejects undefined via concrete guard");

            // Absent optional structural read during base ctor does not trigger descendant narrowing check
            // (both for no-slot constructor writes and for `starts_absent` concrete subclass slots).
            const litSub = new InitLitSub();
            assert(litSub.early === undefined && litSub.earlyNum === undefined && litSub.x === 1, "InitLitSub: absent structural reads skip descendant check");
            const concLitSub = new ConcreteLitSub();
            assert(concLitSub.early === undefined && concLitSub.earlyNum === undefined && concLitSub.x === 1, "ConcreteLitSub: starts_absent structural reads skip descendant check");

            // Narrowed abstract property whose check omits `declaration` still compiles and enforces static check.
            const leg = new LegacyNoDeclSub();
            assert(leg.readLegacy() === 9, "LegacyNoDeclSub: initial read");
            const legBase: Base = leg;
            legBase.setX("bad-legacy");
            let legThrew = false;
            try {
                leg.readLegacy();
            } catch (e) {
                legThrew = e instanceof TypeError;
            }
            assert(legThrew, "LegacyNoDeclSub: static narrowing check throws TypeError");

            // Multi-field (2 abstract + 1 concrete) + multi-level generic narrowing + bind_receiver after grow_fields.
            const ml = new MultiLeaf();
            const mlBase: MultiBase = ml;
            const mlMidWide: MultiMid<number | string> = ml;
            const mlSubWide: MultiSub<number> = ml;
            assert(ml.readA() === 1 && ml.readB() === 1 && ml.readC() === 1 && ml.readSubA() === 1, "MultiLeaf: initial reads");
            // `a = 5` is valid for MultiMid<number> but invalid for MultiSub<1 | 2>.
            mlBase.setAll(5, "bad-b", "bad-c");
            assert(ml.readA() === 5 && readMultiMidA(mlMidWide) === 5, "MultiMid<number> accepts 5 after grow_fields");
            let subAThrew = false;
            try {
                readMultiSubA(mlSubWide);
            } catch (e) {
                subAThrew = e instanceof TypeError;
            }
            assert(subAThrew, "MultiSub<1 | 2> rejects 5 via its own context-row guard");
            let midBThrew = false;
            try {
                readMultiMidB(mlMidWide);
            } catch (e) {
                midBThrew = e instanceof TypeError;
            }
            assert(midBThrew, "MultiMid second abstract guard (b) rejects string");
            let midCThrew = false;
            try {
                readMultiMidC(mlMidWide);
            } catch (e) {
                midCThrew = e instanceof TypeError;
            }
            assert(midCThrew, "MultiMid concrete guard (c) rejects string");
            let bindRecvThrew = false;
            try {
                ml.readB();
            } catch (e) {
                bindRecvThrew = e instanceof TypeError;
            }
            assert(bindRecvThrew, "MultiMid::readB bind_receiver still works after grow_fields");

            // Narrowed abstract intermediate enforces guard on constructor-written subclass,
            // including catching absent reads during base construction as TypeError (not Wasm trap).
            const il = new InitLeaf();
            assert(il.absentThrew, "NarrowMid reading absent abstract property throws catchable TypeError");
            const bIl: Base = il;
            bIl.setX("nope");
            assert(bIl.readBase() === "nope", "InitLeaf base view reads wide value");
            let midThrew = false;
            try {
                il.readNarrow();
            } catch (e) {
                midThrew = e instanceof TypeError;
            }
            assert(midThrew, "InitLeaf narrowed abstract view throws TypeError");

            // Postfix unary on an unnarrowed abstract base does not spuriously enforce descendant narrowing.
            const wn: WideNum = new NarrowNum();
            wn.x = 5;
            assert(wn.bump() === 5 && wn.x === 6, "postfix on unnarrowed abstract base skips descendant check");

            // Uninitialized optional concrete field over abstract property resets base ctor write.
            const sub = new OptResetSub();
            assert(sub.opt === undefined && "opt" in sub && Object.hasOwn(sub, "opt"), "OptResetSub resets base write");

            // Dynamic bracket reads, Readonly<T> dynamic reads, and Record casts enforce abstract guards.
            let bracketLeafThrew = false;
            try {
                readMidKey<number | string>(wideMidLeaf, "x");
            } catch (e) {
                bracketLeafThrew = e instanceof TypeError;
            }
            assert(bracketLeafThrew, "Leaf: dynamic m[k] enforces abstract guard");

            let readonlyLeafThrew = false;
            try {
                readReadonlyMid(leaf);
            } catch (e) {
                readonlyLeafThrew = e instanceof TypeError;
            }
            assert(readonlyLeafThrew, "Leaf: Readonly<T>.x throws catchable TypeError (not trap)");

            let recordLeafThrew = false;
            try {
                const _r = (leaf as unknown) as Record<string, unknown>;
                assert(_r["x"] === -1);
            } catch (e) {
                recordLeafThrew = e instanceof TypeError;
            }
            assert(recordLeafThrew, "Leaf: Record cast enforces abstract guard");

            let bracketInitThrew = false;
            try {
                readMidKey<number | string>(wideInitLeaf, "x");
            } catch (e) {
                bracketInitThrew = e instanceof TypeError;
            }
            assert(bracketInitThrew, "InitGenericLeaf: dynamic m[k] enforces abstract guard");

            let recordInitThrew = false;
            try {
                const _r = (initLeaf as unknown) as Record<string, unknown>;
                assert(_r["x"] === -1);
            } catch (e) {
                recordInitThrew = e instanceof TypeError;
            }
            assert(recordInitThrew, "InitGenericLeaf: Record cast enforces abstract guard");

            // Concrete subclass further narrowing an already-narrowed abstract ancestor field.
            const cna = new ConcreteNarrowOverAbstract();
            const cnaBase: Base = cna;
            const cnaMid: NarrowMid = cna;
            cnaBase.setX(5);
            assert(cnaMid.readNarrow() === 5, "ConcreteNarrowOverAbstract: NarrowMid accepts 5");
            let cnaLeafThrew = false;
            try {
                const _bad = cna.x;
                assert(_bad === 1);
            } catch (e) {
                cnaLeafThrew = e instanceof TypeError;
            }
            assert(cnaLeafThrew, "ConcreteNarrowOverAbstract: concrete slot guard rejects 5");
            cnaBase.setX("bad");
            let cnaMidThrew = false;
            try {
                cnaMid.readNarrow();
            } catch (e) {
                cnaMidThrew = e instanceof TypeError;
            }
            assert(cnaMidThrew, "ConcreteNarrowOverAbstract: abstract context guard rejects string");

            // Optional method override on an unrelated class still reads closure when another class narrows `f`.
            const _nf = new NarrowFSub();
            assert(_nf.f === 1);
            const optMethodView: { f?: () => number } = new OptMethodSub();
            assert(optMethodView.f !== undefined && optMethodView.f() === 42, "OptMethodSub: structural read preserves optional method override");

            // Deleting an optional abstract property (concrete starts_absent slot & dynamic slot) restores absent state.
            const dConc = new DeleteConcreteOptSub();
            const dConc2 = new DeleteConcreteOptSub();
            assert("x" in dConc && Object.hasOwn(dConc, "x") && Object.keys(dConc).includes("x"), "DeleteConcreteOptSub: present before delete");
            deleteField(dConc, "x");
            deleteField(dConc2, "x");
            assert(!("x" in dConc) && !Object.hasOwn(dConc, "x") && !Object.keys(dConc).includes("x"), "DeleteConcreteOptSub: absent after delete");
            const dConcView: { x?: number } = dConc;
            assert(dConcView.x === undefined && dConc === dConc2, "DeleteConcreteOptSub: structural read & equals after delete");
            const dConcMap = new Map<DeleteConcreteOptSub, string>();
            dConcMap.set(dConc, "del");
            assert(dConcMap.get(dConc2) === "del", "DeleteConcreteOptSub: Map hash after delete");

            const dDyn = new DeleteDynamicOptSub();
            const dDyn2 = new DeleteDynamicOptSub();
            assert("x" in dDyn && Object.hasOwn(dDyn, "x") && Object.keys(dDyn).includes("x"), "DeleteDynamicOptSub: present before delete");
            deleteField(dDyn, "x");
            deleteField(dDyn2, "x");
            assert(!("x" in dDyn) && !Object.hasOwn(dDyn, "x") && !Object.keys(dDyn).includes("x"), "DeleteDynamicOptSub: absent after delete");
            const dDynView: { x?: number } = dDyn;
            assert(dDynView.x === undefined && dDyn === dDyn2, "DeleteDynamicOptSub: structural read & equals after delete");
            const dDynMap = new Map<DeleteDynamicOptSub, string>();
            dDynMap.set(dDyn, "del");
            assert(dDynMap.get(dDyn2) === "del", "DeleteDynamicOptSub: Map hash after delete");

            return "ok";
        }
    "#;
    assert_eq!(
        run(source, |ast| {
            mark_class_field_abstract(ast, "Root", "value");
            mark_class_field_abstract(ast, "Base", "x");
            mark_class_field_abstract(ast, "Mid", "x");
            mark_class_field_abstract(ast, "BaseMaybe", "x");
            mark_class_field_abstract(ast, "MidOpt", "x");
            mark_class_field_abstract(ast, "MidStrict", "x");
            // Remove MidStrict's placeholder `this.x = dummy;` so LazyStrictLeaf leaves x absent.
            let mid_strict_body = ast.types.iter().find_map(|decl| {
                if let submilli_engine::TypedTypeDecl::Class(class) = decl
                    && class.name.name == "MidStrict"
                {
                    class.constructor.as_ref().map(|ctor| ctor.body)
                } else {
                    None
                }
            });
            if let Some(body) = mid_strict_body
                && let submilli_engine::TypedStmtKind::Block(stmts) =
                    &mut ast.try_stmt_mut(body).unwrap().kind
            {
                stmts.truncate(1);
            }
            mark_class_field_abstract(ast, "PeekBase", "x");
            mark_class_field_abstract(ast, "NarrowLitMid", "x");
            mark_class_field_abstract(ast, "LegacyNoDeclMid", "x");
            // Simulate a deserialized or external front-end check omitting `declaration`.
            let mut legacy_mangled = None;
            for decl in &mut ast.types {
                if let submilli_engine::TypedTypeDecl::Class(class) = decl
                    && class.name.name == "LegacyNoDeclMid"
                {
                    for field in &mut class.fields {
                        if field.name.name == "x"
                            && let Some(check) = &mut field.narrowing_check
                        {
                            legacy_mangled = check.declaration.take();
                        }
                    }
                }
            }
            if let Some(mangled) = legacy_mangled {
                for guards in ast.runtime_field_guards.values_mut() {
                    guards.retain(|guard| guard.check.declaration.as_ref() != Some(&mangled));
                }
            }
            mark_class_field_abstract(ast, "MultiBase", "a");
            mark_class_field_abstract(ast, "MultiBase", "b");
            mark_class_field_abstract(ast, "MultiMid", "a");
            mark_class_field_abstract(ast, "MultiMid", "b");
            mark_class_field_abstract(ast, "MultiSub", "a");
            mark_class_field_abstract(ast, "NarrowMid", "x");
            mark_class_field_abstract(ast, "WideNum", "x");
            mark_class_field_abstract(ast, "BaseWrites", "opt");
            mark_class_field_abstract(ast, "NarrowFBase", "f");
            mark_class_field_abstract(ast, "DeleteOptBase", "x");
            rename_class_accessors(ast, &["AccessorGenericLeaf"], "_x", "x");
            let mut deleted = 0;
            for id in ast.expr_ids().unwrap() {
                let expr = ast.try_expr(id).unwrap().clone();
                let TypedExprKind::Call { mangled, args, .. } = expr.kind else {
                    continue;
                };
                if mangled != submilli_engine::mangle::package_symbol("main", "deleteField") {
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
                deleted += 1;
            }
            assert_eq!(deleted, 4);
        }),
        "ok"
    );
}

#[test]
fn abstract_property_methods_errors_and_operators() {
    let source = r#"
        class CallableBase {
            fn: () => number = () => 0;
            inBase: boolean;
            ownInBase: boolean;
            constructor() {
                this.inBase = "fn" in this;
                this.ownInBase = Object.hasOwn(this, "fn");
            }
            invoke(): number { return this.fn(); }
        }

        class MethodSub extends CallableBase {
            tag: number = 1;
            _fn(): number { return 42 + this.tag; }
        }

        class AbstractToStringBase {
            toString: () => string = () => "";
        }

        class CustomToStringSub extends AbstractToStringBase {
            label: string = "custom";
            _toString(): string { return "str:" + this.label; }
        }

        class DynToStringSub extends AbstractToStringBase {
            label: string = "dyn";
            constructor() {
                super();
                this.toString = () => "dyn:" + this.label;
            }
        }

        class AbstractErrBase extends Error {
            code: number = 0;
            inBase: boolean;
            constructor(msg: string) {
                super(msg);
                this.inBase = "code" in this;
            }
            readCode(): number { return this.code; }
        }

        class ConcreteErrSub extends AbstractErrBase {
            code: number = 503;
        }

        class OpsBase {
            x: number | undefined = 0;
            fillDefault(d: number): void { this.x = this.x ?? d; }
        }

        class OpsNumBase {
            n: number = 0;
            addToN(d: number): void { this.n += d; }
        }

        class OpsSub extends OpsBase {
            y: number = 10;
            constructor() {
                super();
                this.fillDefault(5);
            }
        }

        class OpsNumSub extends OpsNumBase {
            y: number = 2;
            constructor() {
                super();
                this.n = 7;
            }
        }

        function main(): string {
            // Subclass method implementing abstract property.
            const m1 = new MethodSub();
            const m2 = new MethodSub();
            assert(m1.inBase && !m1.ownInBase, "MethodSub: prototype method visible via in, not own");
            assert("fn" in m1 && !Object.hasOwn(m1, "fn"), "MethodSub: after ctor still prototype method");
            assert(!Object.keys(m1).includes("fn"), "MethodSub: Object.keys omits method");
            assert(m1.invoke() === 43, "MethodSub: invoke through base");
            assert(m1 === m2, "MethodSub: equal instances");
            const mMap = new Map<MethodSub, string>();
            mMap.set(m1, "ok");
            assert(mMap.get(m2) === "ok", "MethodSub: Map hash matches");

            // Abstract toString implemented by subclass method or dynamic constructor assignment.
            const ts = new CustomToStringSub();
            const tsBase: AbstractToStringBase = ts;
            assert(tsBase.toString() === "str:custom", "CustomToStringSub: toString through base");
            assert(String(ts) === "str:custom" && `${ts}` === "str:custom", "CustomToStringSub: String()/template coercion");
            const dynTs = new DynToStringSub();
            assert(String(dynTs) === "dyn:dyn" && `${dynTs}` === "dyn:dyn", "DynToStringSub: dynamic toString coercion");

            // Error subclass with abstract property preserves RefEq and identity hashing.
            const e1 = new ConcreteErrSub("boom");
            const e2 = new ConcreteErrSub("boom");
            assert(!e1.inBase && e1.readCode() === 503 && Object.hasOwn(e1, "code"), "ConcreteErrSub: absent in base, present after init");
            assert(e1 === e1 && e1 !== e2, "ConcreteErrSub: Error uses RefEq");
            const eMap = new Map<ConcreteErrSub, number>();
            eMap.set(e1, 1);
            assert(eMap.get(e1) === 1 && eMap.get(e2) === undefined, "ConcreteErrSub: Error identity hash");

            // Compound assignment, nullish fallback, destructuring, for..in, entries/values.
            const ops = new OpsSub();
            assert(ops.x === 5 && Object.hasOwn(ops, "x"), "OpsSub: ?? defines dynamic field");
            ops.fillDefault(99);
            assert(ops.x === 5, "OpsSub: ?? keeps existing value");
            const { x } = ops;
            assert(x === 5, "OpsSub: destructuring reads dynamic abstract field");

            const on = new OpsNumSub();
            on.addToN(3);
            assert(on.n === 10, "OpsNumSub: += updates dynamic abstract field");
            const forInKeys: string[] = [];
            for (const k in on) {
                forInKeys.push(k);
            }
            assert(forInKeys.join(",") === "y,n", "OpsNumSub: for..in enumerates y then dynamic n");
            assert(Object.entries(on).length === 2, "OpsNumSub: Object.entries length");
            assert(Object.values(on).length === 2, "OpsNumSub: Object.values length");

            return "ok";
        }
    "#;
    assert_eq!(
        run(source, |ast| {
            mark_class_field_abstract(ast, "CallableBase", "fn");
            rename_class_method(ast, "MethodSub", "_fn", "fn");
            mark_class_field_abstract(ast, "AbstractToStringBase", "toString");
            rename_class_method(ast, "CustomToStringSub", "_toString", "toString");
            mark_class_field_abstract(ast, "AbstractErrBase", "code");
            mark_class_field_abstract(ast, "OpsBase", "x");
            mark_class_field_abstract(ast, "OpsNumBase", "n");
        }),
        "ok"
    );
}

#[test]
fn abstract_property_absent_reads_without_getters() {
    // Module declares NO getter accessors, verifying absent/reset/setter-only
    // abstract reads, `NonNull`-narrowed abstract properties (with and without
    // generic instance guards), and narrowed `++` never trap: unnarrowed reads
    // yield `undefined`, unnarrowed `++` yields `NaN` (matching JS), and
    // narrowed reads / `++` throw a catchable `TypeError`.
    let source = r#"
        class PrimBase {
            x: number = 0;
            readInBase: unknown = 1;
            bumpInBase: number = 0;
            constructor() {
                this.readInBase = this.x;
                this.bumpInBase = this.x++;
            }
            bump(): number { return this.x++; }
        }

        class PrimSub extends PrimBase {
            x: number = 1;
        }

        class ResetSub extends PrimBase {
            x: number;
            bumpAfterReset: number = 0;
            constructor() {
                super();
                this.bumpAfterReset = this.bump();
                this.x = 5;
            }
        }

        class NarrowBase {
            v: number | string = 0;
        }

        class NarrowMid extends NarrowBase {
            v: number = 0;
            absentThrew: boolean = false;
            constructor() {
                super();
                try {
                    const _v: number = this.v;
                    assert(_v === -1);
                } catch (e) {
                    this.absentThrew = e instanceof TypeError;
                }
            }
        }

        class NarrowSub extends NarrowMid {
            v: number = 3;
        }

        class NullBase {
            n: number | null = null;
            setN(v: number | null): void { this.n = v; }
        }

        class NullMid extends NullBase {
            n: number = 0;
            absentThrew: boolean = false;
            constructor() {
                super();
                try {
                    const _v: number = this.n;
                    assert(_v === -1);
                } catch (e) {
                    this.absentThrew = e instanceof TypeError;
                }
            }
            readN(): number { return this.n; }
            bumpN(): number { return this.n++; }
        }

        class NullConcreteSub extends NullMid {
            n: number = 4;
            bumpOwn(): number { return this.n++; }
        }

        class NullResetSub extends NullMid {
            n: number;
            readAfterResetThrew: boolean = false;
            constructor() {
                super();
                try {
                    const _v: number = this.readN();
                    assert(_v === -1);
                } catch (e) {
                    this.readAfterResetThrew = e instanceof TypeError;
                }
                this.n = 7;
            }
        }

        class GenNullBase<T> {
            n: T | null = null;
            setN(v: T | null): void { this.n = v; }
        }

        class GenNullMid<T> extends GenNullBase<T> {
            n: T;
            absentThrew: boolean = false;
            constructor(init: T) {
                super();
                try {
                    const _v: T = this.readN();
                    assert(_v === init || _v === undefined);
                } catch (e) {
                    this.absentThrew = e instanceof TypeError;
                }
                this.n = init;
            }
            readN(): T { return this.n; }
        }

        class GenNullResetSub extends GenNullMid<number> {
            n: number;
            bracketAfterResetThrew: boolean = false;
            recordCastAfterResetThrew: boolean = false;
            constructor() {
                super(0);
                try {
                    const view: { n: unknown } = this;
                    const key: "n" = "n";
                    const _v = view[key];
                    assert(_v === 0);
                } catch (e) {
                    this.bracketAfterResetThrew = e instanceof TypeError;
                }
                try {
                    const _rec = (this as unknown) as Record<string, unknown>;
                    assert(_rec["n"] === 0);
                } catch (e) {
                    this.recordCastAfterResetThrew = e instanceof TypeError;
                }
                this.n = 11;
            }
        }

        class GenNullableSub extends GenNullMid<number | null> {
            n: number | null = null;
        }

        class GenNullableObjSub extends GenNullMid<object | null> {
            n: object | null = null;
        }

        class GenOptUndefSub extends GenNullMid<number | undefined> {
            n: number | undefined;
        }

        class OptNullBase {
            n?: number | null = null;
            setN(v: number | null | undefined): void { this.n = v; }
        }

        class OptNullMid extends OptNullBase {
            n?: number;
            absentValue: number | undefined = 1;
            constructor() {
                super();
                this.absentValue = this.n;
            }
            readN(): number | undefined { return this.n; }
        }

        class OptNullSub extends OptNullMid {
            n?: number = 8;
        }

        class EmptyObjBase {
            v: {} | null = null;
            setV(v: {} | null): void { this.v = v; }
        }

        class EmptyObjMid extends EmptyObjBase {
            v: {} = 0;
            absentThrew: boolean = false;
            constructor() {
                super();
                try {
                    const _v: {} = this.v;
                    assert(_v === 0);
                } catch (e) {
                    this.absentThrew = e instanceof TypeError;
                }
            }
            readV(): {} { return this.v; }
        }

        class EmptyObjSub extends EmptyObjMid {
            v: {} = 1;
        }

        class NonPrimBase {
            v: object | null = null;
            setV(v: object | null): void { this.v = v; }
        }

        class NonPrimMid extends NonPrimBase {
            v: object = {};
            absentThrew: boolean = false;
            constructor() {
                super();
                try {
                    const _v: object = this.v;
                    assert(_v !== null);
                } catch (e) {
                    this.absentThrew = e instanceof TypeError;
                }
            }
            readV(): object { return this.v; }
        }

        class NonPrimSub extends NonPrimMid {
            v: object = { ok: true };
        }

        type RecNode = { next: RecNode | null };
        type RecNext = NonNullable<RecNode["next"]>;

        class RecAliasBase {
            node: RecNext | null = null;
            setNode(v: RecNext | null): void { this.node = v; }
        }

        class RecAliasMid extends RecAliasBase {
            node: RecNext = { next: null };
            absentThrew: boolean = false;
            constructor() {
                super();
                try {
                    const _v: RecNext = this.node;
                    assert(_v !== null);
                } catch (e) {
                    this.absentThrew = e instanceof TypeError;
                }
            }
            readNode(): RecNext { return this.node; }
        }

        class RecAliasSub extends RecAliasMid {
            node: RecNext = { next: null };
        }

        type Wrap<T> = T | { nested: Wrap<Wrap<T>> };
        type InnerWrap = NonNullable<Wrap<undefined>>["nested"];

        class WrapAliasBase {
            w: InnerWrap | null = null;
            setW(v: InnerWrap | null): void { this.w = v; }
        }

        class WrapAliasMid extends WrapAliasBase {
            w: InnerWrap = undefined;
            absentValue: InnerWrap = undefined;
            constructor() {
                super();
                this.absentValue = this.w;
            }
            readW(): InnerWrap { return this.w; }
        }

        class WrapAliasSub extends WrapAliasMid {
            w: InnerWrap = undefined;
        }

        class NonNullUtilBase<T> {
            v: NonNullable<T> | null = null;
            setV(x: NonNullable<T> | null): void { this.v = x; }
        }

        class NonNullUtilMid<T> extends NonNullUtilBase<T> {
            v: NonNullable<T>;
            absentThrew: boolean = false;
            constructor(init: NonNullable<T>) {
                super();
                try {
                    const _v: NonNullable<T> = this.readV();
                    assert(_v === init);
                } catch (e) {
                    this.absentThrew = e instanceof TypeError;
                }
                this.v = init;
            }
            readV(): NonNullable<T> { return this.v; }
        }

        class NonNullUtilSub extends NonNullUtilMid<number | null> {
            v: number = 9;
        }

        class PickUtilBase<T extends { a: number; b: string }> {
            v: Pick<T, "a"> | null = null;
            setV(x: Pick<T, "a"> | null): void { this.v = x; }
        }

        class PickUtilMid<T extends { a: number; b: string }> extends PickUtilBase<T> {
            v: Pick<T, "a">;
            absentThrew: boolean = false;
            constructor(init: Pick<T, "a">) {
                super();
                try {
                    const _v: Pick<T, "a"> = this.readV();
                    assert(_v === init);
                } catch (e) {
                    this.absentThrew = e instanceof TypeError;
                }
                this.v = init;
            }
            readV(): Pick<T, "a"> { return this.v; }
        }

        class PickUtilSub extends PickUtilMid<{ a: number; b: string }> {
            v: { a: number } = { a: 4 };
        }

        class PickUnionBase<T extends { a: number; b: string }> {
            v: Pick<T, "a"> | number | null = null;
            setV(x: Pick<T, "a"> | number | null): void { this.v = x; }
        }

        class PickUnionMid<T extends { a: number; b: string }> extends PickUnionBase<T> {
            v: Pick<T, "a"> | number;
            absentThrew: boolean = false;
            constructor(init: Pick<T, "a"> | number) {
                super();
                try {
                    const _v: Pick<T, "a"> | number = this.readV();
                    assert(_v === init);
                } catch (e) {
                    this.absentThrew = e instanceof TypeError;
                }
                this.v = init;
            }
            readV(): Pick<T, "a"> | number { return this.v; }
        }

        class PickUnionSub extends PickUnionMid<{ a: number; b: string }> {
            v: { a: number } | number = 7;
        }

        class PartialUtilBase<T> {
            v: Partial<T> | null = null;
            setV(x: Partial<T> | null): void { this.v = x; }
        }

        class PartialUtilMid<T> extends PartialUtilBase<T> {
            v: Partial<T>;
            absentThrew: boolean = false;
            constructor(init: Partial<T>) {
                super();
                try {
                    const _v: Partial<T> = this.readV();
                    assert(_v === init);
                } catch (e) {
                    this.absentThrew = e instanceof TypeError;
                }
                this.v = init;
            }
            readV(): Partial<T> { return this.v; }
        }

        class PartialUnknownSub extends PartialUtilMid<unknown> {
            v: {} = { ok: true };
        }

        class KeyBase<T> {
            k: keyof T | null = null;
            setK(x: keyof T | null): void { this.k = x; }
        }

        class KeyMid<T> extends KeyBase<T> {
            k: keyof T;
            absentThrew: boolean = false;
            constructor(init: keyof T) {
                super();
                try {
                    const _k: keyof T = this.readK();
                    assert(_k === init);
                } catch (e) {
                    this.absentThrew = e instanceof TypeError;
                }
                this.k = init;
            }
            readK(): keyof T { return this.k; }
        }

        class KeySub extends KeyMid<{ a: number; b: string }> {
            k: "a" | "b" = "a";
        }

        class RetUtilBase<F extends () => unknown> {
            v: ReturnType<F> | null = null;
            setV(x: ReturnType<F> | null): void { this.v = x; }
        }

        class RetUtilMid<F extends () => unknown> extends RetUtilBase<F> {
            v: ReturnType<F>;
            absentThrew: boolean = false;
            constructor(init: ReturnType<F>) {
                super();
                try {
                    const _v: ReturnType<F> = this.readV();
                    assert(_v === init);
                } catch (e) {
                    this.absentThrew = e instanceof TypeError;
                }
                this.v = init;
            }
            readV(): ReturnType<F> { return this.v; }
        }

        class RetNumSub extends RetUtilMid<() => number> {
            v: number = 12;
        }

        class RetVoidSub extends RetUtilMid<() => void> {
            v: void = undefined;
        }

        class RetNullableSub extends RetUtilMid<() => number | null> {
            v: number | null = null;
        }

        class ExclUtilBase<T> {
            v: Exclude<T, null | undefined> | null = null;
            setV(x: Exclude<T, null | undefined> | null): void { this.v = x; }
        }

        class ExclUtilMid<T> extends ExclUtilBase<T> {
            v: Exclude<T, null | undefined>;
            absentThrew: boolean = false;
            constructor(init: Exclude<T, null | undefined>) {
                super();
                try {
                    const _v: Exclude<T, null | undefined> = this.readV();
                    assert(_v === init || _v === undefined);
                } catch (e) {
                    this.absentThrew = e instanceof TypeError;
                }
                this.v = init;
            }
            readV(): Exclude<T, null | undefined> { return this.v; }
        }

        class ExclNumSub extends ExclUtilMid<number | null | undefined> {
            v: number = 13;
        }

        class ExclUnknownSub extends ExclUtilMid<unknown> {}

        class ExclVoidSub extends ExclUtilMid<void> {
            v: void = undefined;
        }

        class ExtrUtilBase<T> {
            v: Extract<T, number> | null = null;
            setV(x: Extract<T, number> | null): void { this.v = x; }
        }

        class ExtrUtilMid<T> extends ExtrUtilBase<T> {
            v: Extract<T, number>;
            absentThrew: boolean = false;
            constructor(init: Extract<T, number>) {
                super();
                try {
                    const _v: Extract<T, number> = this.readV();
                    assert(_v === init);
                } catch (e) {
                    this.absentThrew = e instanceof TypeError;
                }
                this.v = init;
            }
            readV(): Extract<T, number> { return this.v; }
        }

        class ExtrNumSub extends ExtrUtilMid<number | null> {
            v: number = 14;
        }

        class FlatUtilBase<A> {
            v: FlatArray<A, 1> | null = null;
            setV(x: FlatArray<A, 1> | null): void { this.v = x; }
        }

        class FlatUtilMid<A> extends FlatUtilBase<A> {
            v: FlatArray<A, 1>;
            absentThrew: boolean = false;
            constructor(init: FlatArray<A, 1>) {
                super();
                try {
                    const _v: FlatArray<A, 1> = this.readV();
                    assert(_v === init);
                } catch (e) {
                    this.absentThrew = e instanceof TypeError;
                }
                this.v = init;
            }
            readV(): FlatArray<A, 1> { return this.v; }
        }

        class FlatNumSub extends FlatUtilMid<number[][]> {
            v: number = 15;
        }

        class FlatZeroBase<A> {
            v: FlatArray<A, 0> | null = null;
            setV(x: FlatArray<A, 0> | null): void { this.v = x; }
        }

        class FlatZeroMid<A> extends FlatZeroBase<A> {
            v: FlatArray<A, 0>;
            absentThrew: boolean = false;
            constructor(init: FlatArray<A, 0>) {
                super();
                try {
                    const _v: FlatArray<A, 0> = this.readV();
                    assert(_v === init || _v === undefined);
                } catch (e) {
                    this.absentThrew = e instanceof TypeError;
                }
                this.v = init;
            }
            readV(): FlatArray<A, 0> { return this.v; }
        }

        class FlatZeroSub extends FlatZeroMid<(number | null)[][]> {
            v: (number | null)[] = [1];
        }

        class FlatOptTupleSub extends FlatZeroMid<[number, string?]> {
            v: number | string | undefined = undefined;
        }

        type RecPick<T extends { a: number; b: string }> = Pick<T, "a"> | { next: RecPick<T> };

        class RecPickBase<T extends { a: number; b: string }> {
            v: RecPick<T> | null = null;
            setV(x: RecPick<T> | null): void { this.v = x; }
        }

        class RecPickMid<T extends { a: number; b: string }> extends RecPickBase<T> {
            v: RecPick<T>;
            absentThrew: boolean = false;
            constructor(init: RecPick<T>) {
                super();
                try {
                    const _v: RecPick<T> = this.readV();
                    assert(_v === init);
                } catch (e) {
                    this.absentThrew = e instanceof TypeError;
                }
                this.v = init;
            }
            readV(): RecPick<T> { return this.v; }
        }

        class RecPickSub extends RecPickMid<{ a: number; b: string }> {
            v: RecPick<{ a: number; b: string }> = { a: 8 };
        }

        // Composed utilities + parameterized factory guard (`field_guards::body`).
        class NestedExclBase<T> {
            v: Exclude<Exclude<T, undefined>, null> | null = null;
            setV(x: Exclude<Exclude<T, undefined>, null> | null): void { this.v = x; }
        }

        class NestedExclMid<T> extends NestedExclBase<T> {
            v: Exclude<Exclude<T, undefined>, null>;
            absentThrew: boolean = false;
            constructor(init: Exclude<Exclude<T, undefined>, null>) {
                super();
                try {
                    const _v: Exclude<Exclude<T, undefined>, null> = this.readV();
                    assert(_v === init);
                } catch (e) {
                    this.absentThrew = e instanceof TypeError;
                }
                this.v = init;
            }
            readV(): Exclude<Exclude<T, undefined>, null> { return this.v; }
        }

        class NestedExclSub extends NestedExclMid<number | null | undefined> {
            v: number = 16;
        }

        class PartialRetBase<F extends () => unknown> {
            v: Partial<ReturnType<F>> | null = null;
            setV(x: Partial<ReturnType<F>> | null): void { this.v = x; }
        }

        class PartialRetMid<F extends () => unknown> extends PartialRetBase<F> {
            v: Partial<ReturnType<F>>;
            absentThrew: boolean = false;
            constructor(init: Partial<ReturnType<F>>) {
                super();
                try {
                    const _v: Partial<ReturnType<F>> = this.readV();
                    assert(_v === init);
                } catch (e) {
                    this.absentThrew = e instanceof TypeError;
                }
                this.v = init;
            }
            readV(): Partial<ReturnType<F>> { return this.v; }
        }

        class PartialRetUnknownSub extends PartialRetMid<() => unknown> {
            v: {} = { ok: true };
        }

        class FlatNegOneBase<A> {
            v: FlatArray<A, -1> | null = null;
            setV(x: FlatArray<A, -1> | null): void { this.v = x; }
        }

        class FlatNegOneMid<A> extends FlatNegOneBase<A> {
            v: FlatArray<A, -1>;
            absentThrew: boolean = false;
            constructor(init: FlatArray<A, -1>) {
                super();
                try {
                    const _v: FlatArray<A, -1> = this.readV();
                    assert(_v === init);
                } catch (e) {
                    this.absentThrew = e instanceof TypeError;
                }
                this.v = init;
            }
            readV(): FlatArray<A, -1> { return this.v; }
        }

        class FlatNegOneSub extends FlatNegOneMid<null[]> {
            v: null[] = [null];
        }

        class GenNonNullWrap<U> extends GenNullMid<NonNullable<U>> {}

        function makeGenNonNullWrap<U>(init: NonNullable<U>): GenNonNullWrap<U> {
            return new GenNonNullWrap<U>(init);
        }

        class BigIntNullBase {
            b: bigint | null = null;
            setB(v: bigint | null): void { this.b = v; }
        }

        class BigIntNullMid extends BigIntNullBase {
            b: bigint = 0n;
            bumpB(): bigint { return this.b++; }
        }

        class BigIntNullSub extends BigIntNullMid {
            b: bigint = 2n;
            bumpOwn(): bigint { return this.b++; }
        }

        class NullWriteOnlySub extends NullMid {
            last: number = 0;
            set _n(v: number) { this.last = v; }
        }

        class WriteOnlyBase {
            w: number = 0;
            readW(): number { return this.w; }
            bumpW(): number { return this.w++; }
        }

        class WriteOnlySub extends WriteOnlyBase {
            last: number = 0;
            set _w(v: number) { this.last = v; }
        }

        function assertThrowsTypeError(fn: () => void, msg: string): void {
            let threw = false;
            try {
                fn();
            } catch (e) {
                threw = e instanceof TypeError;
            }
            assert(threw, msg);
        }

        function main(): string {
            const p = new PrimSub();
            assert(
                p.readInBase === undefined && Number.isNaN(p.bumpInBase) && p.x === 1,
                "PrimSub: absent read yields undefined, absent ++ yields NaN before subclass init"
            );

            const r = new ResetSub();
            assert(
                r.readInBase === undefined
                    && Number.isNaN(r.bumpInBase)
                    && Number.isNaN(r.bumpAfterReset)
                    && r.x === 5,
                "ResetSub: ++ after FieldSetup::Reset yields NaN without trapping"
            );

            const n = new NarrowSub();
            assert(n.absentThrew && n.v === 3, "NarrowSub: narrowed absent read throws TypeError");

            // FieldNarrowingTest::NonNull must reject absent/reset `$Undefined`
            // (both unguarded and with generic instance guards) and `null` on `++`.
            const nc = new NullConcreteSub();
            assert(nc.absentThrew && nc.readN() === 4, "NullConcreteSub: NonNull rejects absent undefined");
            const nb: NullBase = nc;
            nb.setN(null);
            assertThrowsTypeError(
                () => { nc.bumpN(); },
                "NullConcreteSub: abstract ++ on NonNull-narrowed null throws TypeError"
            );
            assertThrowsTypeError(
                () => { nc.bumpOwn(); },
                "NullConcreteSub: direct ++ on NonNull-narrowed null throws TypeError"
            );

            const nr = new NullResetSub();
            assert(
                nr.absentThrew && nr.readAfterResetThrew && nr.readN() === 7,
                "NullResetSub: NonNull rejects absent and FieldSetup::Reset undefined"
            );

            const gnr = new GenNullResetSub();
            assert(
                gnr.absentThrew
                    && gnr.bracketAfterResetThrew
                    && gnr.recordCastAfterResetThrew
                    && gnr.readN() === 11,
                "GenNullResetSub: guarded NonNull rejects absent undefined, m[k], and Record cast after FieldSetup::Reset"
            );

            // `number | null` (structural guard) and `object | null` (NonNull (true, false)):
            // accept `null`, reject absent `$Undefined`.
            const gns = new GenNullableSub(null);
            assert(
                gns.absentThrew && gns.readN() === null,
                "GenNullableSub: accepts null and rejects absent undefined"
            );
            const gnos = new GenNullableObjSub(null);
            assert(
                gnos.absentThrew && gnos.readN() === null,
                "GenNullableObjSub: NonNull (true, false) accepts null and rejects absent undefined"
            );

            // (false, true): `number | undefined` accepts absent/reset `undefined`, rejects `null`.
            const gou = new GenOptUndefSub(undefined);
            assert(
                !gou.absentThrew && gou.readN() === undefined,
                "GenOptUndefSub: accepts absent/reset undefined"
            );
            const gouBase: GenNullBase<number | undefined> = gou;
            gouBase.setN(null);
            assertThrowsTypeError(
                () => { gou.readN(); },
                "GenOptUndefSub: rejects null"
            );

            const ons = new OptNullSub();
            assert(
                ons.absentValue === undefined && ons.readN() === 8,
                "OptNullSub: optional NonNull (false, true) accepts absent undefined"
            );
            const onsBase: OptNullBase = ons;
            onsBase.setN(null);
            assertThrowsTypeError(
                () => { ons.readN(); },
                "OptNullSub: optional NonNull (false, true) rejects null"
            );

            // (false, false) on `{}` and `object`: reject both `null` and `$Undefined`.
            const eos = new EmptyObjSub();
            assert(eos.absentThrew && eos.readV() === 1, "EmptyObjSub: {} rejects absent undefined");
            const eosBase: EmptyObjBase = eos;
            eosBase.setV(null);
            assertThrowsTypeError(
                () => { eos.readV(); },
                "EmptyObjSub: {} rejects null"
            );

            const nps = new NonPrimSub();
            assert(nps.absentThrew, "NonPrimSub: object rejects absent undefined");
            const npsBase: NonPrimBase = nps;
            npsBase.setV(null);
            assertThrowsTypeError(
                () => { nps.readV(); },
                "NonPrimSub: object rejects null"
            );

            // Genuine `Type::AliasRef` cases: recursive back-edge, nested generic
            // recursive alias (`Wrap<Wrap<undefined>>`), and deferred utilities.
            const ras = new RecAliasSub();
            const rasNode: RecNode = ras.readNode();
            assert(ras.absentThrew && rasNode.next === null, "RecAliasSub: recursive AliasRef rejects absent undefined");
            const rasBase: RecAliasBase = ras;
            rasBase.setNode(null);
            assertThrowsTypeError(
                () => { ras.readNode(); },
                "RecAliasSub: recursive AliasRef rejects null"
            );

            const was = new WrapAliasSub();
            assert(
                was.absentValue === undefined && was.readW() === undefined,
                "WrapAliasSub: nested generic AliasRef Wrap<Wrap<undefined>> accepts undefined"
            );
            const wasBase: WrapAliasBase = was;
            wasBase.setW(null);
            assertThrowsTypeError(
                () => { was.readW(); },
                "WrapAliasSub: nested generic AliasRef Wrap<Wrap<undefined>> rejects null"
            );

            const nnus = new NonNullUtilSub(9);
            assert(nnus.absentThrew && nnus.readV() === 9, "NonNullUtilSub: NonNullable<T> rejects absent undefined");
            const nnusBase: NonNullUtilBase<number | null> = nnus;
            nnusBase.setV(null);
            assertThrowsTypeError(
                () => { nnus.readV(); },
                "NonNullUtilSub: NonNullable<T> rejects null"
            );

            const pus = new PickUtilSub({ a: 4 });
            assert(pus.absentThrew && pus.readV().a === 4, "PickUtilSub: Pick<T, 'a'> rejects absent undefined");
            const pusBase: PickUtilBase<{ a: number; b: string }> = pus;
            pusBase.setV(null);
            assertThrowsTypeError(
                () => { pus.readV(); },
                "PickUtilSub: Pick<T, 'a'> rejects null"
            );

            // Utility inside a union (`Pick<T, 'a'> | number`), `Partial<unknown>`,
            // `keyof T`, `ReturnType`, `Exclude`, `Extract`, and `FlatArray`.
            const puv = new PickUnionSub(7);
            assert(puv.absentThrew && puv.readV() === 7, "PickUnionSub: Pick<T, 'a'> | number rejects absent undefined");
            const puvBase: PickUnionBase<{ a: number; b: string }> = puv;
            puvBase.setV(null);
            assertThrowsTypeError(
                () => { puv.readV(); },
                "PickUnionSub: Pick<T, 'a'> | number rejects null"
            );

            const punks = new PartialUnknownSub({});
            assert(punks.absentThrew, "PartialUnknownSub: Partial<unknown> ({}) rejects absent undefined");
            const punksBase: PartialUtilBase<unknown> = punks;
            punksBase.setV(null);
            assertThrowsTypeError(
                () => { punks.readV(); },
                "PartialUnknownSub: Partial<unknown> ({}) rejects null"
            );

            const ks = new KeySub("a");
            assert(ks.absentThrew && ks.readK() === "a", "KeySub: keyof T rejects absent undefined");
            const ksBase: KeyBase<{ a: number; b: string }> = ks;
            ksBase.setK(null);
            assertThrowsTypeError(
                () => { ks.readK(); },
                "KeySub: keyof T rejects null"
            );

            const rns = new RetNumSub(12);
            assert(rns.absentThrew && rns.readV() === 12, "RetNumSub: ReturnType<() => number> rejects absent undefined");
            const rnsBase: RetUtilBase<() => number> = rns;
            rnsBase.setV(null);
            assertThrowsTypeError(
                () => { rns.readV(); },
                "RetNumSub: ReturnType<() => number> rejects null"
            );

            const rvs = new RetVoidSub(undefined);
            assert(!rvs.absentThrew && rvs.readV() === undefined, "RetVoidSub: ReturnType<() => void> accepts undefined");
            const rvsBase: RetUtilBase<() => void> = rvs;
            rvsBase.setV(null);
            assertThrowsTypeError(
                () => { rvs.readV(); },
                "RetVoidSub: ReturnType<() => void> rejects null"
            );

            const rnulls = new RetNullableSub(null);
            assert(rnulls.absentThrew && rnulls.readV() === null, "RetNullableSub: ReturnType<() => number | null> accepts null, rejects absent undefined");

            const ens = new ExclNumSub(13);
            assert(ens.absentThrew && ens.readV() === 13, "ExclNumSub: Exclude<T, null | undefined> rejects absent undefined");
            const ensBase: ExclUtilBase<number | null | undefined> = ens;
            ensBase.setV(null);
            assertThrowsTypeError(
                () => { ens.readV(); },
                "ExclNumSub: Exclude<T, null | undefined> rejects null"
            );

            const eus = new ExclUnknownSub(null);
            assert(!eus.absentThrew && eus.readV() === null, "ExclUnknownSub: Exclude<unknown, null | undefined> accepts null and undefined");

            const evs = new ExclVoidSub(undefined);
            assert(!evs.absentThrew && evs.readV() === undefined, "ExclVoidSub: Exclude<void, null | undefined> accepts undefined");
            const evsBase: ExclUtilBase<void> = evs;
            evsBase.setV(null);
            assertThrowsTypeError(
                () => { evs.readV(); },
                "ExclVoidSub: Exclude<void, null | undefined> rejects null"
            );

            const xns = new ExtrNumSub(14);
            assert(xns.absentThrew && xns.readV() === 14, "ExtrNumSub: Extract<T, number> rejects absent undefined");
            const xnsBase: ExtrUtilBase<number | null> = xns;
            xnsBase.setV(null);
            assertThrowsTypeError(
                () => { xns.readV(); },
                "ExtrNumSub: Extract<T, number> rejects null"
            );

            const fns = new FlatNumSub(15);
            assert(fns.absentThrew && fns.readV() === 15, "FlatNumSub: FlatArray<number[][], 1> rejects absent undefined");
            const fnsBase: FlatUtilBase<number[][]> = fns;
            fnsBase.setV(null);
            assertThrowsTypeError(
                () => { fns.readV(); },
                "FlatNumSub: FlatArray<number[][], 1> rejects null"
            );

            const fzs = new FlatZeroSub([1]);
            assert(fzs.absentThrew && fzs.readV()[0] === 1, "FlatZeroSub: FlatArray<(number | null)[][], 0> rejects absent undefined");
            const fzsBase: FlatZeroBase<(number | null)[][]> = fzs;
            fzsBase.setV(null);
            assertThrowsTypeError(
                () => { fzs.readV(); },
                "FlatZeroSub: FlatArray<(number | null)[][], 0> rejects null"
            );

            const fots = new FlatOptTupleSub(undefined);
            assert(!fots.absentThrew && fots.readV() === undefined, "FlatOptTupleSub: FlatArray<[number, string?], 0> accepts undefined");
            const fotsBase: FlatZeroBase<[number, string?]> = fots;
            fotsBase.setV(null);
            assertThrowsTypeError(
                () => { fots.readV(); },
                "FlatOptTupleSub: FlatArray<[number, string?], 0> rejects null"
            );

            const rps = new RecPickSub({ a: 8 });
            assert(rps.absentThrew, "RecPickSub: recursive alias wrapping Pick rejects absent undefined");
            const rpsBase: RecPickBase<{ a: number; b: string }> = rps;
            rpsBase.setV(null);
            assertThrowsTypeError(
                () => { rps.readV(); },
                "RecPickSub: recursive alias wrapping Pick rejects null"
            );

            // Composed utilities + parameterized factory guard (`field_guards::body`).
            const nes = new NestedExclSub(16);
            assert(nes.absentThrew && nes.readV() === 16, "NestedExclSub: Exclude<Exclude<T, undefined>, null> rejects absent undefined");
            const nesBase: NestedExclBase<number | null | undefined> = nes;
            nesBase.setV(null);
            assertThrowsTypeError(
                () => { nes.readV(); },
                "NestedExclSub: Exclude<Exclude<T, undefined>, null> rejects null"
            );

            const prus = new PartialRetUnknownSub({});
            assert(prus.absentThrew, "PartialRetUnknownSub: Partial<ReturnType<() => unknown>> rejects absent undefined");
            const prusBase: PartialRetBase<() => unknown> = prus;
            prusBase.setV(null);
            assertThrowsTypeError(
                () => { prus.readV(); },
                "PartialRetUnknownSub: Partial<ReturnType<() => unknown>> rejects null"
            );

            const fnos = new FlatNegOneSub([null]);
            assert(fnos.absentThrew && fnos.readV().length === 1, "FlatNegOneSub: FlatArray<null[], -1> rejects absent undefined");
            const fnosBase: FlatNegOneBase<null[]> = fnos;
            fnosBase.setV(null);
            assertThrowsTypeError(
                () => { fnos.readV(); },
                "FlatNegOneSub: FlatArray<null[], -1> rejects null"
            );

            const gnnw = makeGenNonNullWrap<number | null>(21);
            assert(gnnw.absentThrew && gnnw.readN() === 21, "GenNonNullWrap: parameterized NonNullable<U> guard rejects absent undefined");
            const gnnwBase: GenNullBase<NonNullable<number | null>> = gnnw;
            gnnwBase.setN(null);
            assertThrowsTypeError(
                () => { gnnw.readN(); },
                "GenNonNullWrap: parameterized NonNullable<U> guard rejects null"
            );

            // `bigint | null` -> `bigint` postfix `++` rejects `null` on abstract and concrete slots.
            const bns = new BigIntNullSub();
            assert(bns.bumpB() === 2n && bns.bumpOwn() === 3n && bns.b === 4n, "BigIntNullSub: valid ++");
            const bnsBase: BigIntNullBase = bns;
            bnsBase.setB(null);
            assertThrowsTypeError(
                () => { bns.bumpB(); },
                "BigIntNullSub: abstract bigint ++ rejects null"
            );
            assertThrowsTypeError(
                () => { bns.bumpOwn(); },
                "BigIntNullSub: direct bigint ++ rejects null"
            );

            const nwo = new NullWriteOnlySub();
            assert(nwo.absentThrew, "NullWriteOnlySub: setter-only NonNull rejects absent undefined");

            const wo = new WriteOnlySub();
            const woBase: WriteOnlyBase = wo;
            woBase.w = 9;
            assert(wo.last === 9, "WriteOnlySub: setter invoked");
            const woRead: unknown = woBase.readW();
            assert(woRead === undefined, "WriteOnlySub: getter-less read returns undefined");
            const woBump = woBase.bumpW();
            assert(
                Number.isNaN(woBump) && Number.isNaN(wo.last),
                "WriteOnlySub: ++ without any getter yields NaN and calls setter"
            );

            return "ok";
        }
    "#;
    assert_eq!(
        run(source, |ast| {
            mark_class_field_abstract(ast, "PrimBase", "x");
            mark_class_field_abstract(ast, "NarrowBase", "v");
            mark_class_field_abstract(ast, "NarrowMid", "v");
            mark_class_field_abstract(ast, "NullBase", "n");
            mark_class_field_abstract(ast, "NullMid", "n");
            mark_class_field_abstract(ast, "GenNullBase", "n");
            mark_class_field_abstract(ast, "GenNullMid", "n");
            mark_class_field_abstract(ast, "OptNullBase", "n");
            mark_class_field_abstract(ast, "OptNullMid", "n");
            mark_class_field_abstract(ast, "EmptyObjBase", "v");
            mark_class_field_abstract(ast, "EmptyObjMid", "v");
            mark_class_field_abstract(ast, "NonPrimBase", "v");
            mark_class_field_abstract(ast, "NonPrimMid", "v");
            mark_class_field_abstract(ast, "RecAliasBase", "node");
            mark_class_field_abstract(ast, "RecAliasMid", "node");
            mark_class_field_abstract(ast, "WrapAliasBase", "w");
            mark_class_field_abstract(ast, "WrapAliasMid", "w");
            mark_class_field_abstract(ast, "NonNullUtilBase", "v");
            mark_class_field_abstract(ast, "NonNullUtilMid", "v");
            mark_class_field_abstract(ast, "PickUtilBase", "v");
            mark_class_field_abstract(ast, "PickUtilMid", "v");
            mark_class_field_abstract(ast, "PickUnionBase", "v");
            mark_class_field_abstract(ast, "PickUnionMid", "v");
            mark_class_field_abstract(ast, "PartialUtilBase", "v");
            mark_class_field_abstract(ast, "PartialUtilMid", "v");
            mark_class_field_abstract(ast, "KeyBase", "k");
            mark_class_field_abstract(ast, "KeyMid", "k");
            mark_class_field_abstract(ast, "RetUtilBase", "v");
            mark_class_field_abstract(ast, "RetUtilMid", "v");
            mark_class_field_abstract(ast, "ExclUtilBase", "v");
            mark_class_field_abstract(ast, "ExclUtilMid", "v");
            mark_class_field_abstract(ast, "ExtrUtilBase", "v");
            mark_class_field_abstract(ast, "ExtrUtilMid", "v");
            mark_class_field_abstract(ast, "FlatUtilBase", "v");
            mark_class_field_abstract(ast, "FlatUtilMid", "v");
            mark_class_field_abstract(ast, "FlatZeroBase", "v");
            mark_class_field_abstract(ast, "FlatZeroMid", "v");
            mark_class_field_abstract(ast, "RecPickBase", "v");
            mark_class_field_abstract(ast, "RecPickMid", "v");
            mark_class_field_abstract(ast, "NestedExclBase", "v");
            mark_class_field_abstract(ast, "NestedExclMid", "v");
            mark_class_field_abstract(ast, "PartialRetBase", "v");
            mark_class_field_abstract(ast, "PartialRetMid", "v");
            mark_class_field_abstract(ast, "FlatNegOneBase", "v");
            mark_class_field_abstract(ast, "FlatNegOneMid", "v");
            mark_class_field_abstract(ast, "BigIntNullBase", "b");
            mark_class_field_abstract(ast, "BigIntNullMid", "b");
            rename_class_accessors(ast, &["NullWriteOnlySub"], "_n", "n");
            mark_class_field_abstract(ast, "WriteOnlyBase", "w");
            rename_class_accessors(ast, &["WriteOnlySub"], "_w", "w");
        }),
        "ok"
    );
}

#[test]
fn abstract_property_non_null_utility_narrowing() {
    let source = r#"
        class GenNullBase<T> {
            n: T | null = null;
            setN(v: T | null): void { this.n = v; }
        }

        class GenNullMid<T> extends GenNullBase<T> {
            n: T;
            absentThrew: boolean = false;
            constructor(init: T) {
                super();
                try {
                    const _v: T = this.readN();
                    assert(_v === init || _v === undefined);
                } catch (e) {
                    this.absentThrew = e instanceof TypeError;
                }
                this.n = init;
            }
            readN(): T { return this.n; }
        }

        class GenUnknownSub extends GenNullMid<unknown> {}
        class GenEmptyObjSub extends GenNullMid<{}> {}
        class GenObjectSub extends GenNullMid<object> {}

        class FlatZeroBase<A> {
            v: FlatArray<A, 0> | null = null;
            setV(x: FlatArray<A, 0> | null): void { this.v = x; }
        }

        class FlatZeroMid<A> extends FlatZeroBase<A> {
            v: FlatArray<A, 0>;
            absentThrew: boolean = false;
            constructor(init: FlatArray<A, 0>) {
                super();
                try {
                    const _v: FlatArray<A, 0> = this.readV();
                    assert(_v === init || _v === undefined);
                } catch (e) {
                    this.absentThrew = e instanceof TypeError;
                }
                this.v = init;
            }
            readV(): FlatArray<A, 0> { return this.v; }
        }

        class FlatNonNullArrSub<T> extends FlatZeroMid<NonNullable<T>> {}
        class FlatReadonlySub<T> extends FlatZeroMid<Readonly<T>> {}
        class FlatPartialSub<T> extends FlatZeroMid<Partial<T>> {}
        class FlatParamsSub<F extends (...args: never[]) => unknown> extends FlatZeroMid<Parameters<F>> {}

        class PartialRetBase<F extends () => unknown> {
            v: Partial<ReturnType<F>> | null = null;
            setV(x: Partial<ReturnType<F>> | null): void { this.v = x; }
        }

        class PartialRetMid<F extends () => unknown> extends PartialRetBase<F> {
            v: Partial<ReturnType<F>>;
            absentThrew: boolean = false;
            constructor(init: Partial<ReturnType<F>>) {
                super();
                try {
                    const _v: Partial<ReturnType<F>> = this.readV();
                    assert(_v === init);
                } catch (e) {
                    this.absentThrew = e instanceof TypeError;
                }
                this.v = init;
            }
            readV(): Partial<ReturnType<F>> { return this.v; }
        }

        class PartialRetUnionSub extends PartialRetMid<(() => unknown) | (() => null)> {}

        class RetNonNullBase<F extends (() => unknown) | null> {
            v: ReturnType<NonNullable<F>> | null = null;
            setV(x: ReturnType<NonNullable<F>> | null): void { this.v = x; }
        }

        class RetNonNullMid<F extends (() => unknown) | null> extends RetNonNullBase<F> {
            v: ReturnType<NonNullable<F>>;
            absentThrew: boolean = false;
            constructor(init: ReturnType<NonNullable<F>>) {
                super();
                try {
                    const _v: ReturnType<NonNullable<F>> = this.readV();
                    assert(_v === init);
                } catch (e) {
                    this.absentThrew = e instanceof TypeError;
                }
                this.v = init;
            }
            readV(): ReturnType<NonNullable<F>> { return this.v; }
        }

        class RetNonNullSub extends RetNonNullMid<(() => number) | null> {}

        class RetGenBase<F extends (...args: never[]) => unknown> {
            v: ReturnType<F> | null = null;
            setV(x: ReturnType<F> | null): void { this.v = x; }
        }

        class RetGenMid<F extends (...args: never[]) => unknown> extends RetGenBase<F> {
            v: ReturnType<F>;
            absentThrew: boolean = false;
            constructor(init: ReturnType<F>) {
                super();
                try {
                    const _v: ReturnType<F> = this.readV();
                    assert(_v === init);
                } catch (e) {
                    this.absentThrew = e instanceof TypeError;
                }
                this.v = init;
            }
            readV(): ReturnType<F> { return this.v; }
        }

        type GenNumFn = <X extends number>(x: X) => X;
        class RetGenericFnSub extends RetGenMid<GenNumFn> {}

        class ExclRetBase<T, F extends () => unknown> {
            v: Exclude<T, ReturnType<F>> | null = null;
            setV(x: Exclude<T, ReturnType<F>> | null): void { this.v = x; }
        }

        class ExclRetMid<T, F extends () => unknown> extends ExclRetBase<T, F> {
            v: Exclude<T, ReturnType<F>>;
            absentThrew: boolean = false;
            constructor(init: Exclude<T, ReturnType<F>>) {
                super();
                try {
                    const _v: Exclude<T, ReturnType<F>> = this.readV();
                    assert(_v === init);
                } catch (e) {
                    this.absentThrew = e instanceof TypeError;
                }
                this.v = init;
            }
            readV(): Exclude<T, ReturnType<F>> { return this.v; }
        }

        class ExclRetNullSub extends ExclRetMid<number | null, () => null> {}

        // Non-generic `unknown -> {}` narrowing (Shape({}) alongside `Type::Unknown`
        // in `runtime_type_tests`), plus deferred `Exclude`/`Extract`/`FlatArray`
        // monotonicity and union-top (`{} | null | undefined`) edge cases.
        class UnknownBase {
            v: unknown = null;
            setV(x: unknown): void { this.v = x; }
        }

        class EmptyObjFromUnknownSub extends UnknownBase {
            v: {} = { ok: true };
            readV(): {} { return this.v; }
        }

        class PartialExclBase<T> {
            v: Partial<Exclude<unknown, T> | null> | null = null;
            setV(x: Partial<Exclude<unknown, T> | null> | null): void { this.v = x; }
        }

        class PartialExclMid<T> extends PartialExclBase<T> {
            v: Partial<Exclude<unknown, T> | null>;
            absentThrew: boolean = false;
            constructor(init: Partial<Exclude<unknown, T> | null>) {
                super();
                try {
                    const _v: Partial<Exclude<unknown, T> | null> = this.readV();
                    assert(_v === init || _v === undefined);
                } catch (e) {
                    this.absentThrew = e instanceof TypeError;
                }
                this.v = init;
            }
            readV(): Partial<Exclude<unknown, T> | null> { return this.v; }
        }

        class PartialExclUnknownSub extends PartialExclMid<unknown> {}

        class PartialFlatDepthBase<D extends number> {
            v: Partial<FlatArray<unknown[], D> | null> | null = null;
            setV(x: Partial<FlatArray<unknown[], D> | null> | null): void { this.v = x; }
        }

        class PartialFlatDepthMid<D extends number> extends PartialFlatDepthBase<D> {
            v: Partial<FlatArray<unknown[], D> | null>;
            constructor(init: Partial<FlatArray<unknown[], D> | null>) {
                super();
                this.v = init;
            }
            readV(): Partial<FlatArray<unknown[], D> | null> { return this.v; }
        }

        class PartialFlatNegOneSub extends PartialFlatDepthMid<-1> {}

        class ExclUnionTopBase<F extends () => unknown> {
            v: Exclude<ReturnType<F>, {} | null | undefined> | null = null;
            setV(x: Exclude<ReturnType<F>, {} | null | undefined> | null): void { this.v = x; }
        }

        class ExclUnionTopMid<F extends () => unknown> extends ExclUnionTopBase<F> {
            v: Exclude<ReturnType<F>, {} | null | undefined>;
            absentThrew: boolean = false;
            constructor(init: Exclude<ReturnType<F>, {} | null | undefined>) {
                super();
                try {
                    const _v: Exclude<ReturnType<F>, {} | null | undefined> = this.readV();
                    assert(_v === init || _v === undefined);
                } catch (e) {
                    this.absentThrew = e instanceof TypeError;
                }
                this.v = init;
            }
            readV(): Exclude<ReturnType<F>, {} | null | undefined> { return this.v; }
        }

        class ExclUnionTopVoidSub extends ExclUnionTopMid<() => void> {}

        class ExtrMayTopBase<U> {
            v: Extract<unknown, NonNullable<U> | null | undefined> | null = null;
            setV(x: Extract<unknown, NonNullable<U> | null | undefined> | null): void { this.v = x; }
        }

        class ExtrMayTopMid<U> extends ExtrMayTopBase<U> {
            v: Extract<unknown, NonNullable<U> | null | undefined>;
            absentThrew: boolean = false;
            constructor(init: Extract<unknown, NonNullable<U> | null | undefined>) {
                super();
                try {
                    const _v: Extract<unknown, NonNullable<U> | null | undefined> = this.readV();
                    assert(_v === init || _v === undefined);
                } catch (e) {
                    this.absentThrew = e instanceof TypeError;
                }
                this.v = init;
            }
            readV(): Extract<unknown, NonNullable<U> | null | undefined> { return this.v; }
        }

        class ExtrMayTopUnknownSub extends ExtrMayTopMid<unknown> {}

        // Antitone `Extract` inside `Exclude`, union-top requiring `undefined`
        // rather than `void`, and recursive `AliasRef` inside `Extract`/`ReturnType`.
        class ExclExtrNullBase<U> {
            v: Exclude<null, Extract<unknown, null | U>> | null = null;
            setV(x: Exclude<null, Extract<unknown, null | U>> | null): void { this.v = x; }
        }

        class ExclExtrNullMid<U> extends ExclExtrNullBase<U> {
            v: Exclude<null, Extract<unknown, null | U>>;
            constructor(init: Exclude<null, Extract<unknown, null | U>>) {
                super();
                this.v = init;
            }
            readV(): Exclude<null, Extract<unknown, null | U>> { return this.v; }
        }

        class ExclExtrNullNumSub extends ExclExtrNullMid<number> {}

        class ExclVoidUnionBase<T> {
            v: Exclude<T, {} | null | void> | null = null;
            setV(x: Exclude<T, {} | null | void> | null): void { this.v = x; }
        }

        class ExclVoidUnionMid<T> extends ExclVoidUnionBase<T> {
            v: Exclude<T, {} | null | void>;
            constructor(init: Exclude<T, {} | null | void>) {
                super();
                this.v = init;
            }
            readV(): Exclude<T, {} | null | void> { return this.v; }
        }

        class ExclVoidUnionUnknownSub extends ExclVoidUnionMid<unknown> {}

        type WrapNull<T> = T | { nested: WrapNull<WrapNull<T>> };
        type InnerNull = NonNullable<WrapNull<null>>["nested"];

        class ExclExtrRecBase<T> {
            v: Exclude<null, Extract<T, null>> | null = null;
            setV(x: Exclude<null, Extract<T, null>> | null): void { this.v = x; }
        }

        class ExclExtrRecMid<T> extends ExclExtrRecBase<T> {
            v: Exclude<null, Extract<T, null>>;
            constructor(init: Exclude<null, Extract<T, null>>) {
                super();
                this.v = init;
            }
            readV(): Exclude<null, Extract<T, null>> { return this.v; }
        }

        class ExclExtrRecSub extends ExclExtrRecMid<InnerNull> {}

        class ExclRetRecBase<F extends () => unknown> {
            v: Exclude<unknown, {} | ReturnType<F> | undefined> | null = null;
            setV(x: Exclude<unknown, {} | ReturnType<F> | undefined> | null): void { this.v = x; }
        }

        class ExclRetRecMid<F extends () => unknown> extends ExclRetRecBase<F> {
            v: Exclude<unknown, {} | ReturnType<F> | undefined>;
            constructor(init: Exclude<unknown, {} | ReturnType<F> | undefined>) {
                super();
                this.v = init;
            }
            readV(): Exclude<unknown, {} | ReturnType<F> | undefined> { return this.v; }
        }

        class ExclRetRecSub extends ExclRetRecMid<() => InnerNull> {}

        // Non-monotonicity of homomorphic utilities in `unknown`:
        // `Partial<T | null>` and `FlatArray<Readonly<T | unknown[]>, 0>` at `T = unknown`.
        class ExclPartialBase<T> {
            v: Exclude<null, Partial<T | null>> | null = null;
            setV(x: Exclude<null, Partial<T | null>> | null): void { this.v = x; }
        }

        class ExclPartialMid<T> extends ExclPartialBase<T> {
            v: Exclude<null, Partial<T | null>>;
            constructor(init: Exclude<null, Partial<T | null>>) {
                super();
                this.v = init;
            }
            readV(): Exclude<null, Partial<T | null>> { return this.v; }
        }

        class ExclPartialUnknownSub extends ExclPartialMid<unknown> {}

        class PartialFlatReadonlyUnionBase<T> {
            v: Partial<FlatArray<Readonly<T | unknown[]>, 0> | null> | null = null;
            setV(x: Partial<FlatArray<Readonly<T | unknown[]>, 0> | null> | null): void { this.v = x; }
        }

        class PartialFlatReadonlyUnionMid<T> extends PartialFlatReadonlyUnionBase<T> {
            v: Partial<FlatArray<Readonly<T | unknown[]>, 0> | null>;
            constructor(init: Partial<FlatArray<Readonly<T | unknown[]>, 0> | null>) {
                super();
                this.v = init;
            }
            readV(): Partial<FlatArray<Readonly<T | unknown[]>, 0> | null> { return this.v; }
        }

        class PartialFlatReadonlyUnionSub extends PartialFlatReadonlyUnionMid<unknown> {}

        // Multi-depth `FlatArray` union (`-1 | 0`) widens `flat_predecessor(-1)` to
        // all depths, unwrapping all nested array layers.
        class FlatUnionDepthBase<A> {
            v: FlatArray<A, -1 | 0> | null = null;
            setV(x: FlatArray<A, -1 | 0> | null): void { this.v = x; }
        }

        class FlatUnionDepthMid<A> extends FlatUnionDepthBase<A> {
            v: FlatArray<A, -1 | 0>;
            constructor(init: FlatArray<A, -1 | 0>) {
                super();
                this.v = init;
            }
            readV(): FlatArray<A, -1 | 0> { return this.v; }
        }

        class FlatUnionDepthNullSub extends FlatUnionDepthMid<null[][]> {}

        function assertThrowsTypeError(fn: () => void, msg: string): void {
            let threw = false;
            try {
                fn();
            } catch (e) {
                threw = e instanceof TypeError;
            }
            assert(threw, msg);
        }

        function main(): string {
            // Instantiate `GenNullMid<unknown>` first so `runtime_type_tests`
            // has `Type::Unknown => Shape(Type::Unknown)`, then verify `{}` and
            // `object` (which also lower to `Type::Unknown`) still enforce `NonNull`.
            const gu = new GenUnknownSub(null);
            assert(!gu.absentThrew && gu.readN() === null, "GenNullMid<unknown> accepts null and undefined");

            const geo = new GenEmptyObjSub({ ok: true });
            assert(geo.absentThrew, "GenNullMid<{}> rejects absent undefined");
            const geoBase: GenNullBase<{}> = geo;
            geoBase.setN(null);
            assertThrowsTypeError(() => { geo.readN(); }, "GenNullMid<{}> rejects null");

            const go = new GenObjectSub({ ok: true });
            assert(go.absentThrew, "GenNullMid<object> rejects absent undefined");
            const goBase: GenNullBase<object> = go;
            goBase.setN(null);
            assertThrowsTypeError(() => { go.readN(); }, "GenNullMid<object> rejects null");

            // `FlatArray` over `NonNullable`, `Readonly`, `Partial`, and `Parameters`.
            const fnna = new FlatNonNullArrSub<(number | null)[] | null>(null);
            assert(fnna.absentThrew && fnna.readV() === null, "FlatArray<NonNullable<(number | null)[] | null>, 0> accepts null, rejects undefined");

            const fnn = new FlatNonNullArrSub<number[] | null>(5);
            assert(fnn.absentThrew && fnn.readV() === 5, "FlatArray<NonNullable<number[] | null>, 0> rejects absent undefined");
            const fnnBase: FlatZeroBase<NonNullable<number[] | null>> = fnn;
            fnnBase.setV(null);
            assertThrowsTypeError(() => { fnn.readV(); }, "FlatArray<NonNullable<number[] | null>, 0> rejects null");

            const fro = new FlatReadonlySub<unknown>({ ok: true });
            assert(fro.absentThrew, "FlatArray<Readonly<unknown>, 0> rejects absent undefined");
            const froBase: FlatZeroBase<Readonly<unknown>> = fro;
            froBase.setV(null);
            assertThrowsTypeError(() => { fro.readV(); }, "FlatArray<Readonly<unknown>, 0> rejects null");

            const froa = new FlatReadonlySub<unknown[]>(null);
            assert(!froa.absentThrew && froa.readV() === null, "FlatArray<Readonly<unknown[]>, 0> accepts null and undefined");

            const fp = new FlatPartialSub<number[]>(undefined);
            assert(!fp.absentThrew && fp.readV() === undefined, "FlatArray<Partial<number[]>, 0> accepts undefined");

            const fpar = new FlatParamsSub<(x: number | null) => void>(null);
            assert(fpar.readV() === null, "FlatArray<Parameters<(x: number | null) => void>, 0> accepts null");

            // `unknown` union absorption in `Partial<ReturnType<(() => unknown) | (() => null)>>`.
            const pru = new PartialRetUnionSub({ ok: true });
            assert(pru.absentThrew, "Partial<ReturnType<(() => unknown) | (() => null)>> rejects absent undefined");
            const pruBase: PartialRetBase<(() => unknown) | (() => null)> = pru;
            pruBase.setV(null);
            assertThrowsTypeError(() => { pru.readV(); }, "Partial<ReturnType<(() => unknown) | (() => null)>> rejects null");

            // `ReturnType<NonNullable<...>>` and generic function `ReturnType<<X extends number>() => X>`.
            const rnn = new RetNonNullSub(7);
            assert(rnn.absentThrew && rnn.readV() === 7, "ReturnType<NonNullable<(() => number) | null>> rejects absent undefined");
            const rnnBase: RetNonNullBase<(() => number) | null> = rnn;
            rnnBase.setV(null);
            assertThrowsTypeError(() => { rnn.readV(); }, "ReturnType<NonNullable<(() => number) | null>> rejects null");

            const rgf = new RetGenericFnSub(8);
            assert(rgf.absentThrew && rgf.readV() === 8, "ReturnType<<X extends number>(x: X) => X> rejects absent undefined");
            const rgfBase: RetGenBase<GenNumFn> = rgf;
            rgfBase.setV(null);
            assertThrowsTypeError(() => { rgf.readV(); }, "ReturnType<<X extends number>(x: X) => X> rejects null");

            // `Exclude<number | null, ReturnType<() => null>>`.
            const ern = new ExclRetNullSub(9);
            assert(ern.absentThrew && ern.readV() === 9, "Exclude<number | null, ReturnType<() => null>> rejects absent undefined");
            const ernBase: ExclRetBase<number | null, () => null> = ern;
            ernBase.setV(null);
            assertThrowsTypeError(() => { ern.readV(); }, "Exclude<number | null, ReturnType<() => null>> rejects null");

            // Non-generic `unknown -> {}` must not collide with `runtime_type_tests[Type::Unknown]`.
            const eou = new EmptyObjFromUnknownSub();
            const eouBase: UnknownBase = eou;
            eouBase.setV(null);
            assertThrowsTypeError(() => { eou.readV(); }, "unknown -> {} rejects null");
            eouBase.setV(undefined);
            assertThrowsTypeError(() => { eou.readV(); }, "unknown -> {} rejects undefined");

            // `Partial<Exclude<unknown, T> | null>` at `T = unknown` admits `null`.
            const peu = new PartialExclUnknownSub(null);
            assert(peu.readV() === null, "Partial<Exclude<unknown, unknown> | null> accepts null");

            // `Partial<FlatArray<unknown[], D> | null>` at `D = -1` admits `null`.
            const pfn = new PartialFlatNegOneSub(null);
            assert(pfn.readV() === null, "Partial<FlatArray<unknown[], -1> | null> accepts null");

            // `Exclude<ReturnType<() => void>, {} | null | undefined>` is `void` (accepts `undefined`, rejects `null`).
            const eut = new ExclUnionTopVoidSub(undefined);
            assert(!eut.absentThrew && eut.readV() === undefined, "Exclude<void, {} | null | undefined> accepts undefined");
            const eutBase: ExclUnionTopBase<() => void> = eut;
            eutBase.setV(null);
            assertThrowsTypeError(() => { eut.readV(); }, "Exclude<void, {} | null | undefined> rejects null");

            // `Extract<unknown, NonNullable<U> | null | undefined>` at `U = unknown` is `unknown` (accepts `null` and `undefined`).
            const emt = new ExtrMayTopUnknownSub(null);
            assert(!emt.absentThrew && emt.readV() === null, "Extract<unknown, NonNullable<unknown> | null | undefined> accepts null and undefined");

            // `Exclude<null, Extract<unknown, null | number>> = null`.
            const een = new ExclExtrNullNumSub(null);
            assert(een.readV() === null, "Exclude<null, Extract<unknown, null | number>> accepts null");

            // `Exclude<unknown, {} | null | void> = unknown` (Submilli `holds_every_value` requires `undefined`, not `void`).
            const evu = new ExclVoidUnionUnknownSub(null);
            assert(evu.readV() === null, "Exclude<unknown, {} | null | void> accepts null");

            // Recursive `AliasRef` inside `Extract` / `ReturnType` under `Exclude`.
            const eer = new ExclExtrRecSub(null);
            assert(eer.readV() === null, "Exclude<null, Extract<InnerNull, null>> accepts null");

            const err = new ExclRetRecSub(null);
            assert(err.readV() === null, "Exclude<unknown, {} | ReturnType<() => InnerNull> | undefined> accepts null");

            // Non-monotonicity in `unknown` under `Partial` and `FlatArray<Readonly<...>>`.
            const epu = new ExclPartialUnknownSub(null);
            assert(epu.readV() === null, "Exclude<null, Partial<unknown | null>> accepts null");

            const pfru = new PartialFlatReadonlyUnionSub(null);
            assert(pfru.readV() === null, "Partial<FlatArray<Readonly<unknown | unknown[]>, 0> | null> accepts null");

            const fudn = new FlatUnionDepthNullSub(null);
            assert(fudn.readV() === null, "FlatArray<null[][], -1 | 0> unwraps all layers and accepts null");

            return "ok";
        }
    "#;
    assert_eq!(
        run(source, |ast| {
            mark_class_field_abstract(ast, "GenNullBase", "n");
            mark_class_field_abstract(ast, "GenNullMid", "n");
            mark_class_field_abstract(ast, "FlatZeroBase", "v");
            mark_class_field_abstract(ast, "FlatZeroMid", "v");
            mark_class_field_abstract(ast, "PartialRetBase", "v");
            mark_class_field_abstract(ast, "PartialRetMid", "v");
            mark_class_field_abstract(ast, "RetNonNullBase", "v");
            mark_class_field_abstract(ast, "RetNonNullMid", "v");
            mark_class_field_abstract(ast, "RetGenBase", "v");
            mark_class_field_abstract(ast, "RetGenMid", "v");
            mark_class_field_abstract(ast, "ExclRetBase", "v");
            mark_class_field_abstract(ast, "ExclRetMid", "v");
            mark_class_field_abstract(ast, "UnknownBase", "v");
            mark_class_field_abstract(ast, "PartialExclBase", "v");
            mark_class_field_abstract(ast, "PartialExclMid", "v");
            mark_class_field_abstract(ast, "PartialFlatDepthBase", "v");
            mark_class_field_abstract(ast, "PartialFlatDepthMid", "v");
            mark_class_field_abstract(ast, "ExclUnionTopBase", "v");
            mark_class_field_abstract(ast, "ExclUnionTopMid", "v");
            mark_class_field_abstract(ast, "ExtrMayTopBase", "v");
            mark_class_field_abstract(ast, "ExtrMayTopMid", "v");
            mark_class_field_abstract(ast, "ExclExtrNullBase", "v");
            mark_class_field_abstract(ast, "ExclExtrNullMid", "v");
            mark_class_field_abstract(ast, "ExclVoidUnionBase", "v");
            mark_class_field_abstract(ast, "ExclVoidUnionMid", "v");
            mark_class_field_abstract(ast, "ExclExtrRecBase", "v");
            mark_class_field_abstract(ast, "ExclExtrRecMid", "v");
            mark_class_field_abstract(ast, "ExclRetRecBase", "v");
            mark_class_field_abstract(ast, "ExclRetRecMid", "v");
            mark_class_field_abstract(ast, "ExclPartialBase", "v");
            mark_class_field_abstract(ast, "ExclPartialMid", "v");
            mark_class_field_abstract(ast, "PartialFlatReadonlyUnionBase", "v");
            mark_class_field_abstract(ast, "PartialFlatReadonlyUnionMid", "v");
            mark_class_field_abstract(ast, "FlatUnionDepthBase", "v");
            mark_class_field_abstract(ast, "FlatUnionDepthMid", "v");
        }),
        "ok"
    );
}

fn mark_class_field_abstract(ast: &mut TypedAst, class_name: &str, field_name: &str) {
    let mut found = false;
    for decl in &mut ast.types {
        if let submilli_engine::TypedTypeDecl::Class(class) = decl
            && class.name.name == class_name
        {
            for field in &mut class.fields {
                if field.name.name == field_name {
                    field.abstract_property = true;
                    field.initializer = None;
                    found = true;
                }
            }
        }
    }
    assert!(
        found,
        "expected field `{field_name}` on class `{class_name}`"
    );
}

fn rename_class_accessors(ast: &mut TypedAst, class_names: &[&str], from: &str, to: &str) {
    let mut renamed = 0;
    for decl in &mut ast.types {
        if let submilli_engine::TypedTypeDecl::Class(class) = decl
            && class_names.contains(&class.name.name.as_str())
        {
            for accessor in &mut class.accessors {
                match accessor {
                    submilli_engine::TypedClassAccessor::Getter { name, .. }
                    | submilli_engine::TypedClassAccessor::Setter { name, .. }
                        if name.name == from =>
                    {
                        name.name = to.to_string();
                        renamed += 1;
                    }
                    _ => {}
                }
            }
        }
    }
    assert!(
        renamed > 0,
        "expected accessor `{from}` on classes `{class_names:?}`"
    );
}

fn rename_class_method(ast: &mut TypedAst, class_name: &str, from: &str, to: &str) {
    let mut found = false;
    for decl in &mut ast.types {
        if let submilli_engine::TypedTypeDecl::Class(class) = decl
            && class.name.name == class_name
        {
            for method in &mut class.methods {
                if method.name.name == from {
                    method.name.name = to.to_string();
                    found = true;
                }
            }
        }
    }
    assert!(found, "expected method `{from}` on class `{class_name}`");
}

#[test]
fn alternate_front_end_class_method_override_arities_and_rest_conventions() {
    let source = r#"
        class EqRestBase {
            m(x: number): number { return x; }
        }
        class EqRestSub extends EqRestBase {
            m_impl(...xs: number[]): number {
                return xs.length * 100 + xs.reduce((a, b) => a + b, 0);
            }
        }

        class ExpandRestBase {
            m(x: number, y: number, ...zs: number[]): number {
                return x + y + zs.length;
            }
        }
        class ExpandRestSub extends ExpandRestBase {
            m_impl(...all: number[]): number {
                return all.length * 1000 + all.reduce((a, b) => a + b, 0);
            }
        }

        class ConsumeRestBase {
            m(...xs: number[]): number {
                return xs.length;
            }
        }
        class ConsumeRestSub extends ConsumeRestBase {
            m_impl(x: number = 10, ...ys: number[]): number {
                return x * 100 + ys.length * 10 + ys.reduce((a, b) => a + b, 0);
            }
        }

        class RestToFixedBase {
            m(...xs: number[]): number {
                return xs.length;
            }
        }
        class RestToFixedSub extends RestToFixedBase {
            m_impl(x: number = 10, y: number = 20): number {
                return x * 100 + y;
            }
        }

        class FewerBase {
            m(x: number, y: number): number {
                return x + y;
            }
        }
        class FewerSub extends FewerBase {
            m_impl(x: number): number {
                return x * 10;
            }
        }

        class ReactA {
            m(x: number): number { return x; }
        }
        class ReactB extends ReactA {
            m(x: number, y: number = 10): number { return x + y; }
        }
        class ReactC extends ReactB {
            m_impl(x: number): number { return x * 100; }
        }

        class GuardParent {
            v: unknown = null;
            setV(v: unknown): void { this.v = v; }
        }
        class GuardChild<T> extends GuardParent {
            v: T;
            constructor(v: T) { super(); this.v = v; }
            readV(): T { return this.v; }
        }
        function verifyRuntimeT<T>(sample: T, candidate: unknown): T {
            const g = new GuardChild<T>(sample);
            g.setV(candidate);
            return g.readV();
        }

        class GenChainA {
            m(sample: unknown, candidate: unknown): unknown { return candidate; }
        }
        class GenChainB extends GenChainA {
            m_impl<T>(sample: T, candidate: unknown): T {
                return verifyRuntimeT<T>(sample, candidate);
            }
        }
        class GenChainC extends GenChainB {
            m(sample: unknown, candidate: unknown, bonus: number = 5): unknown {
                return (candidate as number) + bonus;
            }
        }

        class GChainBase {
            id<T>(sample: T, candidate: unknown, extra?: number): T {
                return verifyRuntimeT<T>(sample, candidate);
            }
            id_chain(sample: number, candidate: unknown, extra?: number): number {
                return 0;
            }
        }
        class GChainSub extends GChainBase {
            id<T>(sample: T, candidate: unknown, extra: number = 10, tag: string = "ok"): T {
                if (tag.length === 0 || extra !== 10) throw new Error("bad default");
                return verifyRuntimeT<T>(sample, candidate);
            }
        }
        function maybeChain(flag: boolean): GChainBase | undefined {
            return flag ? new GChainSub() : undefined;
        }

        class StrOpt {
            toString_impl(radix?: number): string {
                return radix === undefined ? "opt:default" : `opt:${radix}`;
            }
        }
        class StrRest {
            toString_impl(...args: readonly number[]): string {
                return `rest:${args.length}`;
            }
        }
        class JsonSub {
            v: number = 7;
            toJson_impl(pretty?: boolean): string {
                return pretty === true ? `{"v": ${this.v}}` : `{"v":${this.v}}`;
            }
        }

        function main(): string {
            const eq = new EqRestSub();
            if ((eq as EqRestBase).m(7) !== 107) return "eq-base";
            if (eq.m_impl(1, 2, 3) !== 306) return "eq-sub-3";
            if (eq.m_impl() !== 0) return "eq-sub-0";

            const ex = new ExpandRestSub();
            if ((ex as ExpandRestBase).m(1, 2) !== 2003) return "ex-base-2";
            if ((ex as ExpandRestBase).m(1, 2, 3, 4) !== 4010) return "ex-base-4";
            if (ex.m_impl(5) !== 1005) return "ex-sub-1";

            const cr = new ConsumeRestSub();
            if ((cr as ConsumeRestBase).m() !== 1000) return "cr-base-0";
            if ((cr as ConsumeRestBase).m(3) !== 300) return "cr-base-1";
            if ((cr as ConsumeRestBase).m(3, 4, 5) !== 329) return "cr-base-3";
            if (cr.m_impl(2, 6) !== 216) return "cr-sub-2";

            const rf = new RestToFixedSub();
            if ((rf as RestToFixedBase).m() !== 1020) return "rf-base-0";
            if ((rf as RestToFixedBase).m(4) !== 420) return "rf-base-1";
            if ((rf as RestToFixedBase).m(4, 5, 99) !== 405) return "rf-base-3";
            if (rf.m_impl(7, 8) !== 708) return "rf-sub-2";

            const fw = new FewerSub();
            if ((fw as FewerBase).m(6, 99) !== 60) return "fw-base";
            if (fw.m_impl(6) !== 60) return "fw-sub";

            const rc = new ReactC();
            if ((rc as ReactA).m(3) !== 300) return "rc-as-a";
            if ((rc as ReactB).m(3, 99) !== 300) return "rc-as-b-2";
            if ((rc as ReactB).m(3) !== 300) return "rc-as-b-1";
            if (rc.m_impl(3) !== 300) return "rc-own";

            const gc = new GenChainC();
            if ((gc as GenChainA).m(0, 10) !== 15) return "gc-as-a";
            if ((gc as GenChainB).m_impl<number>(0, 10) !== 15) return "gc-as-b";
            if (gc.m(0, 10, 7) !== 17) return "gc-own";

            const gbOnly: GenChainB = new GenChainB();
            if (gbOnly.m_impl<number>(0, 9) !== 9) return "gb-ok";

            if (maybeChain(true)?.id_chain(0, 42) !== 42) return "chain-some";
            if (maybeChain(false)?.id_chain(0, 42) !== undefined) return "chain-none";
            let chainThrew = false;
            try {
                maybeChain(true)?.id_chain(0, "bad");
            } catch (e) {
                chainThrew = e instanceof TypeError;
            }
            if (!chainThrew) return "chain-cast";

            const so = new StrOpt();
            if (String(so as unknown) !== "opt:default") return "str-opt";
            const sr = new StrRest();
            if (String(sr as unknown) !== "rest:0") return "str-rest";
            const js = new JsonSub();
            if (JSON.stringify(js) !== `{"v":7}`) return "json-sub";

            return "ok";
        }
    "#;
    assert_eq!(
        run(source, |ast| {
            for class_name in [
                "EqRestSub",
                "ExpandRestSub",
                "ConsumeRestSub",
                "RestToFixedSub",
                "FewerSub",
                "ReactC",
                "GenChainB",
            ] {
                rename_class_method(ast, class_name, "m_impl", "m");
            }
            for class_name in ["StrOpt", "StrRest"] {
                rename_class_method(ast, class_name, "toString_impl", "toString");
            }
            rename_class_method(ast, "JsonSub", "toJson_impl", "toJson");
            if let Some(submilli_engine::TypedTypeDecl::Class(c)) = ast
                .types
                .iter_mut()
                .find(|d| matches!(d, submilli_engine::TypedTypeDecl::Class(c) if c.name.name == "GChainBase"))
            {
                c.methods.retain(|m| m.name.name != "id_chain");
            }
            for id in ast.expr_ids().unwrap() {
                let expr = ast.try_expr_mut(id).unwrap();
                match &mut expr.kind {
                    TypedExprKind::MethodCall {
                        member: submilli_engine::Member::Named(ident),
                        ..
                    }
                    | TypedExprKind::GenericMethodCall {
                        member: submilli_engine::Member::Named(ident),
                        ..
                    }
                    | TypedExprKind::FieldAccess { name: ident, .. }
                        if ident.name == "m_impl" =>
                    {
                        ident.name = "m".to_string();
                    }
                    TypedExprKind::OptionalChain { parts, .. } => {
                        for part in parts {
                            if let submilli_engine::TypedChainPart::MethodCall {
                                member: submilli_engine::Member::Named(ident),
                                type_args,
                                ..
                            } = part
                                && ident.name == "id_chain"
                            {
                                ident.name = "id".to_string();
                                *type_args = vec![Type::Number];
                            }
                        }
                    }
                    _ => {}
                }
            }
        }),
        "ok"
    );
}

#[test]
fn class_method_override_with_extra_params_across_package_boundary() {
    let dep_pkg = submilli_engine::compile::compile_package_checked(
        "@test/override_dep",
        submilli_engine::ModulePath::from("lib"),
        &[submilli_engine::compile::PackageSourceModule {
            path: submilli_engine::ModulePath::from("lib"),
            source: r#"
                /** Unimported ancestor class in the dependency package. */
                export class UnimportedBase {
                    /**
                     * Single-arg base method.
                     * @param x Value.
                     * @returns Value.
                     */
                    h(x: number): number { return x; }
                }

                /** Exported leaf over unimported base with extra default parameters. */
                export class ExportedLeaf extends UnimportedBase {
                    /**
                     * Override with two extra defaults.
                     * @param x First value.
                     * @param y Second value.
                     * @param z Third value.
                     * @returns Sum.
                     */
                    h(x: number, y: number = 10, z: number = 20): number {
                        return x + y + z;
                    }
                }

                /** Base class with fixed-arity methods. */
                export class A {
                    /**
                     * Single-arg method.
                     * @param x Value.
                     * @returns Value.
                     */
                    m(x: number): number { return x; }
                    /**
                     * Trailing-alphabetical method to guard slot ordering.
                     * @param x Value.
                     * @returns Value.
                     */
                    z(x: number): number { return x; }
                }

                /** Subclass adding optional/rest parameters and an earlier-alphabetical method. */
                export class B extends A {
                    /**
                     * Earlier-alphabetical own method.
                     * @returns Constant.
                     */
                    a(): number { return 7; }
                    /**
                     * Override with default parameter.
                     * @param x Base value.
                     * @param y Optional addend.
                     * @returns Sum.
                     */
                    m(x: number, y: number = 10): number { return x + y; }
                    /**
                     * Override with rest parameter.
                     * @param x Base value.
                     * @param ys Extra values.
                     * @returns Encoded total.
                     */
                    z(x: number, ...ys: number[]): number {
                        return x * 100 + ys.length * 10 + ys.reduce((acc, v) => acc + v, 0);
                    }
                }

                /** Subclass in producer package inheriting B's convention-changing overrides. */
                export class D extends B {}

                /** Base guard class for runtime `T` descriptor checks. */
                export class GuardParent {
                    v: unknown = null;
                    /**
                     * Sets `v`.
                     * @param v Replacement value.
                     */
                    setV(v: unknown): void { this.v = v; }
                }

                /** Generic narrowed subclass enforcing `T`'s runtime descriptor. */
                export class GuardChild<T> extends GuardParent {
                    v: T;
                    /**
                     * Constructs `GuardChild`.
                     * @param v Initial value.
                     */
                    constructor(v: T) { super(); this.v = v; }
                    /**
                     * Reads `v` with narrowing check.
                     * @returns Guarded `T` value.
                     */
                    readV(): T { return this.v; }
                }

                /** Exported generic-method interface whose concrete subclass is never imported by the consumer. */
                export interface IGBase {
                    /**
                     * Validates `candidate` against `T`.
                     * @param sample Sample `T`.
                     * @param candidate Candidate value.
                     * @returns Checked `T`.
                     */
                    id<T>(sample: T, candidate: unknown): T;
                }

                /** Exported base class implementing `IGBase`. */
                export class GBase implements IGBase {
                    /**
                     * Validates `candidate` against `T`.
                     * @param sample Sample `T`.
                     * @param candidate Candidate value.
                     * @returns Checked `T`.
                     */
                    id<T>(sample: T, candidate: unknown): T {
                        const g = new GuardChild<T>(sample);
                        g.setV(candidate);
                        return g.readV();
                    }
                }

                /** Exported subclass overriding `id<T>` with an extra defaulted parameter. */
                export class GSub extends GBase {
                    /**
                     * Validates `candidate` against `T` with extra defaulted tag.
                     * @param sample Sample `T`.
                     * @param candidate Candidate value.
                     * @param tag Tag.
                     * @returns Checked `T`.
                     */
                    id<T>(sample: T, candidate: unknown, tag: string = "ok"): T {
                        if (tag.length === 0) throw new Error("empty tag");
                        const g = new GuardChild<T>(sample);
                        g.setV(candidate);
                        return g.readV();
                    }
                }

                /**
                 * Factory returning a `GSub` erased to `IGBase`.
                 * @returns Instance of `GSub` as `IGBase`.
                 */
                export function makeIG(): IGBase {
                    return new GSub();
                }

                /** Interface renamed to `toString(radix?: number)` in declaration metadata to test `${x}` / `String(x)`. */
                export interface IFmtOpt {
                    /**
                     * Formats with optional radix.
                     * @param radix Optional radix.
                     * @returns Formatted text.
                     */
                    fmtOpt(radix?: number): string;
                }

                class FmtOptImpl implements IFmtOpt {
                    fmtOpt(radix?: number): string {
                        return radix === undefined ? "opt:undef" : `opt:${radix}`;
                    }
                }

                /**
                 * Factory returning an `IFmtOpt` value with a runtime `toString(radix?: number)`.
                 * @returns `IFmtOpt` instance.
                 */
                export function makeFmtOpt(): IFmtOpt {
                    const inst = new FmtOptImpl();
                    (inst as unknown as { [k: string]: unknown })["toString"] =
                        (radix?: number): string => inst.fmtOpt(radix);
                    return inst;
                }

                /** Interface renamed to `toString(...args: readonly number[])` in declaration metadata. */
                export interface IFmtRest {
                    /**
                     * Formats with rest args.
                     * @param args Extra numbers.
                     * @returns Formatted text.
                     */
                    fmtRest(...args: readonly number[]): string;
                }

                class FmtRestImpl implements IFmtRest {
                    fmtRest(...args: readonly number[]): string {
                        return `rest:${args.length}`;
                    }
                }

                /**
                 * Factory returning an `IFmtRest` value with a runtime `toString(...args)`.
                 * @returns `IFmtRest` instance.
                 */
                export function makeFmtRest(): IFmtRest {
                    const inst = new FmtRestImpl();
                    (inst as unknown as { [k: string]: unknown })["toString"] =
                        (...args: readonly number[]): string => inst.fmtRest(...args);
                    return inst;
                }

                /** Interface renamed to `toString(radix = 10)` in declaration metadata. */
                export interface IFmtDef {
                    /**
                     * Formats with default radix.
                     * @param radix Radix.
                     * @returns Formatted text.
                     */
                    fmtDef(radix?: number): string;
                }

                class FmtDefImpl implements IFmtDef {
                    fmtDef(radix: number = 20): string {
                        return `def:${radix}`;
                    }
                }

                /**
                 * Factory returning an `IFmtDef` value whose runtime default `20` overrides `10`.
                 * @returns `IFmtDef` instance.
                 */
                export function makeFmtDef(): IFmtDef {
                    const inst = new FmtDefImpl();
                    (inst as unknown as { [k: string]: unknown })["toString"] =
                        (radix: number = 20): string => inst.fmtDef(radix);
                    return inst;
                }
            "#,
        }],
        &[],
    )
    .expect("compile override_dep package");

    let mut dep_decl = dep_pkg.declaration;
    for (iface_name, old_method, with_default) in [
        ("IFmtOpt", "fmtOpt", None),
        ("IFmtRest", "fmtRest", None),
        (
            "IFmtDef",
            "fmtDef",
            Some(submilli_engine::DefaultValue::Number(10.0)),
        ),
    ] {
        let sym = dep_decl
            .types
            .values_mut()
            .find(|s| s.name == iface_name)
            .expect("interface symbol");
        if let submilli_engine::TypeKind::Interface { methods, .. } = &mut sym.kind {
            let mut sig = methods.remove(old_method).expect("method sig");
            if let Some(def) = with_default {
                sig.params[0].default = Some(def);
            }
            methods.insert("toString".to_string(), sig);
        }
    }

    let consumer_source = r#"
        import { A, B, D, ExportedLeaf, makeFmtDef, makeFmtOpt, makeFmtRest, makeIG } from "@test/override_dep";

        class LocalInherit extends B {}

        class LocalOverride extends B {
            m(x: number, y: number = 20, z: number = 30): number {
                return super.m(x, y) + z;
            }
        }

        interface IA {
            m(x: number): number;
        }

        function main(): string {
            const b = new B();
            if ((b as A).m(1) !== 11) return "b-as-a-m";
            if (b.m(1, 2) !== 3) return "b-m-2";
            if (b.m(1) !== 11) return "b-m-1";
            if ((b as A).z(2) !== 200) return "b-as-a-z";
            if (b.z(2, 3, 4) !== 227) return "b-z-3";
            if (b.a() !== 7) return "b-a";

            const d = new D();
            if ((d as A).m(5) !== 15) return "d-as-a-m";
            if (d.m(5, 6) !== 11) return "d-m-2";

            const li = new LocalInherit();
            if ((li as A).m(4) !== 14) return "li-as-a-m";
            if ((li as A).z(3) !== 300) return "li-as-a-z";
            if (li.m(4, 5) !== 9) return "li-m-2";
            if (li.z(3, 1, 2) !== 323) return "li-z-3";

            const lo = new LocalOverride();
            if ((lo as A).m(1) !== 51) return "lo-as-a-m";
            if ((lo as B).m(1, 2) !== 33) return "lo-as-b-m-2";
            if ((lo as B).m(1) !== 51) return "lo-as-b-m-1";
            if (lo.m(1, 2, 3) !== 6) return "lo-m-3";

            const iaB: IA = b;
            const iaLo: IA = lo;
            if (iaB.m(8) !== 18) return "ia-b";
            if (iaLo.m(1) !== 51) return "ia-lo";

            const leaf = new ExportedLeaf();
            if (leaf.h(1) !== 31) return "leaf-1";
            if (leaf.h(1, 2, 3) !== 6) return "leaf-3";

            const ig = makeIG();
            if (ig.id<number>(0, 42) !== 42) return "ig-id-42";
            let igThrew = false;
            try {
                ig.id<number>(0, "bad");
            } catch (e) {
                igThrew = e instanceof TypeError;
            }
            if (!igThrew) return "ig-id-cast";

            const fmtOpt = makeFmtOpt();
            if (`${fmtOpt}` !== "opt:undef") return "interp-opt";
            if (String(fmtOpt) !== "opt:undef") return "string-opt";

            const fmtRest = makeFmtRest();
            if (`${fmtRest}` !== "rest:0") return "interp-rest";
            if (String(fmtRest) !== "rest:0") return "string-rest";

            const fmtDef = makeFmtDef();
            if (`${fmtDef}` !== "def:20") return "interp-def-override";
            if (String(fmtDef) !== "def:20") return "string-def-override";

            return "ok";
        }
    "#;

    let output = std::thread::Builder::new()
        .stack_size(submilli_engine::compiler_limits::COMPILER_STACK_BYTES)
        .spawn(move || {
            let compiled = submilli_engine::compile::compile_script_checked(
                consumer_source,
                "consumer.ts",
                FileId(0),
                &[&dep_decl],
                &[],
            )
            .expect("compile consumer script");

            let cfg = RuntimeConfig::default();
            let engine = cfg.engine().unwrap();
            let mut data = submilli_engine::runtime::StoreData::with_tempdir().unwrap();
            data.install_type_info(compiled.type_info);
            let mut store = cfg.store_async(&engine, data).unwrap();
            let mut linker = wasmtime::Linker::new(&engine);
            let dep_module = wasmtime::Module::new(&engine, &dep_pkg.wasm).unwrap();
            pollster::block_on(async {
                submilli_engine::runtime::install_runtime_async(&mut linker, &mut store)
                    .await
                    .unwrap();
                submilli_engine::runtime::install_package_modules_async(
                    &mut linker,
                    &mut store,
                    &[submilli_engine::runtime::LinkedPackageModule {
                        module: &dep_module,
                        declaration: &dep_decl,
                        type_info: &dep_pkg.type_info,
                    }],
                )
                .await
                .unwrap();
                let module = wasmtime::Module::new(&engine, &compiled.wasm).unwrap();
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
    assert_eq!(output, "ok");
}
