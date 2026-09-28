use interpreter::{
    Ast, Expr, ExprKind, FileId, LineIndex, Sources, Span, StmtKind, Type, TypeAnnotationKind,
    TypedAst, TypedExpr, TypedExprKind,
    compiler_error::{CompileError, CompilerFailure, CompilerStage},
    source::SourceError,
};

#[test]
fn checked_source_spans_reject_reversed_cross_file_and_unicode_ranges() {
    let file = FileId(0);
    assert!(Span::new(file, 3, 1).is_err());
    let span = Span::new(file, 0, 2).unwrap();
    assert_eq!(span.text("éx", file).unwrap(), "é");
    assert!(span.text("éx", FileId(1)).is_err());
    assert!(span.merge(Span::at(FileId(1))).is_err());
    assert!(
        span.merge(Span {
            file,
            start: 4,
            end: 1
        })
        .is_err()
    );
    assert!(Span::new(file, 1, 2).unwrap().text("éx", file).is_err());
    assert!(Span::new(file, 0, 4).unwrap().text("éx", file).is_err());
    assert_eq!(
        span.merge(Span::new(file, 2, 3).unwrap())
            .unwrap()
            .text("éx", file)
            .unwrap(),
        "éx"
    );
}

#[test]
fn checked_source_lines_own_text_and_reject_invalid_positions() {
    let index = LineIndex::new("é\r\nx\ry\n").unwrap();
    assert_eq!(index.line_count(), 4);
    assert_eq!(index.line_text(1).unwrap(), "é");
    assert_eq!(index.line_text(2).unwrap(), "x");
    assert_eq!(index.line_text(4).unwrap(), "");
    for offset in [1, 9, u32::MAX] {
        assert!(index.line_col(offset).is_err());
    }
    for (line, col) in [(0, 1), (1, 0), (1, 2), (1, 5), (5, 1), (1, u32::MAX)] {
        assert!(index.byte_offset(line, col).is_err(), "{line}:{col}");
    }
    assert_eq!(index.byte_offset(2, 1).unwrap(), 4);
    assert_eq!(index.line_col(4).unwrap(), (2, 1));
    assert!(index.line_text(0).is_err());
    assert!(index.line_text(u32::MAX).is_err());
}

#[test]
fn checked_source_errors_preserve_classification_and_have_no_false_location() {
    SourceError::check_source_len(SourceError::MAX_SOURCE_BYTES).unwrap();
    let failure = SourceError::check_source_len(SourceError::MAX_SOURCE_BYTES + 1)
        .unwrap_err()
        .into_compiler_failure(CompilerStage::Parse);
    assert!(matches!(failure, CompilerFailure::Limit { span: None, .. }));
    let failure = Span::new(FileId(0), 3, 1)
        .unwrap_err()
        .into_compiler_failure(CompilerStage::Infer);
    assert!(matches!(
        failure,
        CompilerFailure::Internal { span: None, .. }
    ));
    let diagnostics = CompileError::from(failure).into_diagnostics(FileId(0));
    assert_eq!(diagnostics[0].span, Span::at(FileId::COMPILER));
    let (sources, _) = Sources::single("script.ts", "first line").unwrap();
    let rendered = interpreter::diagnostics::render(&diagnostics[0], &sources);
    assert!(rendered.contains("internal compiler failure"));
    assert!(!rendered.contains("first line"));
    assert!(!rendered.contains("script.ts:1:1"));
}

#[test]
fn checked_source_diagnostics_preserve_original_error_on_bad_metadata() {
    let (sources, file) = Sources::single("script.ts", "é\nx").unwrap();
    for span in [
        Span {
            file,
            start: 1,
            end: 2,
        },
        Span {
            file,
            start: 3,
            end: 1,
        },
        Span {
            file,
            start: 0,
            end: 100,
        },
        Span::at(FileId(999)),
    ] {
        let diagnostic = interpreter::Diagnostic {
            severity: interpreter::Severity::Error,
            span,
            message: "original failure".into(),
            help: vec![],
            notes: vec![],
        };
        let rendered = interpreter::diagnostics::render(&diagnostic, &sources);
        assert!(rendered.contains("original failure"));
        assert!(rendered.contains("source context unavailable"));
        assert!(!rendered.contains("script.ts:"));
    }
}

#[test]
fn checked_source_registration_keeps_text_and_file_identity_together() {
    let mut sources = Sources::new();
    let a = sources.add("a.ts", "é").unwrap();
    let b = sources.add("b.ts", "x\ny").unwrap();
    assert_eq!(sources.get(a).unwrap().text(), "é");
    assert!(
        sources
            .get(b)
            .unwrap()
            .span_text(Span::new(a, 0, 1).unwrap())
            .is_err()
    );
    assert!(sources.get(FileId::PRELUDE).is_none());
    assert_eq!(sources.find_path("b.ts").unwrap().0, b);
    assert_eq!(
        sources.get(b).unwrap().line_index().line_text(2).unwrap(),
        "y"
    );
}

#[test]
fn checked_source_inference_rejects_corrupt_type_metadata_before_resolution() {
    let source = "function main(x: number): number { return x; }";
    for corrupt in 0..3 {
        let parsed = interpreter::parse_script(source, FileId(0));
        assert!(!parsed.has_errors());
        // Parse separately because ParsedScript intentionally encapsulates its AST.
        let mut lexer = interpreter::Asi::new(source, FileId(0));
        let mut tokens = Vec::new();
        loop {
            let token = lexer.next_token();
            let eof = matches!(token.kind, interpreter::TokenKind::Eof);
            tokens.push(token);
            if eof {
                break;
            }
        }
        let (mut ast, _) = interpreter::parser::parse_checked(source, tokens, FileId(0)).unwrap();
        let root = ast.top_level[0];
        let StmtKind::Function { params, .. } = &mut ast.try_stmt_mut(root).unwrap().kind else {
            panic!("function");
        };
        let annotation = params[0].ty.as_mut().unwrap();
        match corrupt {
            0 => annotation.span.end = u32::MAX,
            1 => {
                if let TypeAnnotationKind::Name { name, .. } = &mut annotation.kind {
                    name.name = "string".into();
                }
            }
            _ => {
                annotation.kind = TypeAnnotationKind::Qualified {
                    path: vec![],
                    args: vec![],
                }
            }
        }
        let error = interpreter::typechecker::infer::infer_with_transitive_checked(
            source,
            "main",
            &ast,
            &[],
            &[],
        )
        .unwrap_err();
        assert!(matches!(
            error.fatal,
            Some(CompilerFailure::Internal {
                stage: CompilerStage::Infer,
                span: None,
                ..
            })
        ));
        assert!(error.to_string().contains("invalid source span"));
    }
    interpreter::compile::typecheck_checked("function main(): number { return 1; }", FileId(0))
        .unwrap();
}

#[test]
fn checked_source_codegen_rejects_invalid_spans_before_emission() {
    let mut ast = TypedAst::new();
    ast.try_push_expr(TypedExpr {
        kind: TypedExprKind::Number(1.0),
        ty: Type::Number,
        span: Span::new(FileId(9), 0, 0).unwrap(),
    })
    .unwrap();
    let error = interpreter::codegen::codegen_with_type_info("", "script.ts", FileId(0), &ast, &[])
        .unwrap_err();
    assert!(matches!(
        error,
        CompilerFailure::Internal {
            stage: CompilerStage::Codegen,
            span: None,
            ..
        }
    ));
}

#[test]
fn checked_source_direct_ast_validation_checks_unreachable_nodes_too() {
    let mut ast = Ast::new();
    ast.try_push_expr(Expr {
        kind: ExprKind::Null,
        span: Span {
            file: FileId(0),
            start: 1,
            end: 2,
        },
    })
    .unwrap();
    assert!(ast.validate_source("é", FileId(0)).is_err());
}

#[test]
fn checked_source_multiline_unicode_docs_keep_original_positions() {
    let source = "/**\n * @param {string} text description\n * @capability x/op { label: \"x\",\n * other: \"é😀é😀é😀\" }\n */\nfunction main(): number { return 1; }";
    let file = FileId(0);
    let raw = interpreter::doc_comment::RawDoc {
        text: source.split("\nfunction").next().unwrap().into(),
        span: Span::new(file, 0, source.find("\nfunction").unwrap() as u32).unwrap(),
    };
    let doc = interpreter::doc_comment::parse_doc_comment(&raw).unwrap();
    assert_eq!(doc.params[0].name_span.text(source, file).unwrap(), "text");
    let capability = &doc.capabilities[0];
    assert_eq!(
        capability.bindings[1]
            .field_span
            .text(source, file)
            .unwrap(),
        "other"
    );
    let interpreter::DocCapabilityBindingKind::Literal { span, .. } = capability.bindings[1].kind
    else {
        panic!("literal");
    };
    assert_eq!(span.text(source, file).unwrap(), "\"é😀é😀é😀\"");
    let parsed = interpreter::parse_script(source, file);
    assert!(!parsed.has_errors(), "{parsed:?}");
}

#[test]
fn checked_source_doc_unicode_errors_and_empty_first_lines_keep_valid_spans() {
    for source in [
        "/**\n * @capability\n * x/op { other: \"é😀\" }\n */\nfunction main(): number { return 1; }",
        "/** @capability x/op { 😀 } */\nfunction main(): number { return 1; }",
    ] {
        let file = FileId(0);
        let ast = parse_ast(source, file);
        ast.validate_source(source, file).unwrap();
    }
}

#[test]
fn checked_source_auxiliary_spans_are_validated() {
    use interpreter::ast::{
        ArrayLiteralElement, ExportedDecl, Ident, ObjectLiteralMember, PatternOrigin,
    };
    let file = FileId(0);
    let bad = Span {
        file: FileId(99),
        start: 2,
        end: 1,
    };
    for kind in 0..5 {
        let mut ast = Ast::new();
        let value = ast
            .try_push_expr(Expr {
                kind: ExprKind::Null,
                span: Span::at(file),
            })
            .unwrap();
        let statement = ast
            .try_push_stmt(interpreter::Stmt {
                kind: StmtKind::Block(vec![]),
                span: Span::at(file),
            })
            .unwrap();
        match kind {
            0 => {
                ast.try_push_expr(Expr {
                    kind: ExprKind::ArrayLiteral {
                        elements: vec![ArrayLiteralElement::Spread { value, span: bad }],
                    },
                    span: Span::at(file),
                })
                .unwrap();
            }
            1 => {
                ast.try_push_expr(Expr {
                    kind: ExprKind::ObjectLiteral {
                        members: vec![ObjectLiteralMember::Spread { value, span: bad }],
                    },
                    span: Span::at(file),
                })
                .unwrap();
            }
            2 => ast.exported_decls.push(ExportedDecl {
                stmt: statement,
                export_span: bad,
            }),
            3 => {
                ast.pattern_origins.insert(
                    value,
                    PatternOrigin {
                        pattern_span: bad,
                        slot_arity: 1,
                    },
                );
            }
            _ => {
                ast.for_of_pattern_bindings.insert(
                    statement,
                    vec![Ident {
                        name: "x".into(),
                        span: bad,
                    }],
                );
            }
        }
        assert!(ast.validate_source("", file).is_err(), "case {kind}");
    }
}

#[test]
fn checked_source_package_validates_before_import_graph_errors() {
    let source = "import { x } from './missing'; export function f(): number { return 1; }";
    let (sources, file) = Sources::single("index", source).unwrap();
    let mut ast = parse_ast(source, file);
    let import = ast.top_level[0];
    let StmtKind::Import { module_span, .. } = &mut ast.try_stmt_mut(import).unwrap().kind else {
        panic!("import");
    };
    *module_span = Span {
        file: FileId(999),
        start: 5,
        end: 1,
    };
    let error = interpreter::typechecker::infer::infer_package_checked(
        "test",
        "index".into(),
        vec![("index".into(), file, &ast)],
        &sources,
        Default::default(),
        Default::default(),
    )
    .unwrap_err();
    assert!(matches!(
        error.fatal,
        Some(CompilerFailure::Internal {
            stage: CompilerStage::Infer,
            ..
        })
    ));
    let error = interpreter::typechecker::infer::infer_package_checked(
        "test",
        "index".into(),
        vec![("index".into(), file, &ast)],
        &Sources::new(),
        Default::default(),
        Default::default(),
    )
    .unwrap_err();
    assert!(matches!(
        error.fatal,
        Some(CompilerFailure::Internal {
            stage: CompilerStage::Infer,
            ..
        })
    ));
    assert!(error.to_string().contains("module source is missing"));
}

#[test]
fn checked_source_validates_predicate_outer_span() {
    let source = "function isString(x: unknown): x is string { return typeof x === 'string'; }";
    let file = FileId(0);
    let mut ast = parse_ast(source, file);
    let function = ast.top_level[0];
    let StmtKind::Function { type_predicate, .. } = &mut ast.try_stmt_mut(function).unwrap().kind
    else {
        panic!("function");
    };
    type_predicate.as_mut().unwrap().span.end = u32::MAX;
    assert!(ast.validate_source(source, file).is_err());
}

fn parse_ast(source: &str, file: FileId) -> Ast {
    let mut lexer = interpreter::Asi::new(source, file);
    let mut tokens = Vec::new();
    loop {
        let token = lexer.next_token();
        let eof = matches!(token.kind, interpreter::TokenKind::Eof);
        tokens.push(token);
        if eof {
            break;
        }
    }
    interpreter::parser::parse_checked(source, tokens, file)
        .unwrap()
        .0
}
