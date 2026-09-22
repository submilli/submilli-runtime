use crate::{
    ArrayLiteralElement, ArrowBody, Ast, BinOp, Binding, CatchClause, Diagnostic, EnumInitializer,
    EnumMember, ExportedDecl, Expr, ExprId, ExprKind, FileId, Ident, ImportKind, ImportSpecifier,
    ObjectLiteralField, ObjectLiteralMember, ObjectPatternField, ParamDecl, PostfixOp, Severity,
    Span, Stmt, StmtId, StmtKind, SwitchCase, SwitchDefault, Token, TokenKind, TypeAnnotation,
    TypeAnnotationField, TypeAnnotationKind, UnOp,
};

const MAX_ERRORS: usize = 20;

/// Where a type is being parsed. Only one production depends on it: `(T)`, spelled
/// identically as a grouped type and as v1's rejected bare parameter list, is decided by
/// whether a `=>` follows the matching `)`. In an arrow's return annotation that `=>` is
/// the *body's*, so the tie-break has no signal there and `(T)` groups — which loses
/// nothing, since a bare parameter list is rejected outright in every position.
#[derive(Clone, Copy, PartialEq)]
enum TypePos {
    Anywhere,
    ArrowReturn,
}

pub fn parse(source: &str, tokens: Vec<Token>, file: FileId) -> (Ast, Vec<Diagnostic>) {
    let mut p = Parser {
        source,
        tokens,
        file,
        pos: 0,
        ast: Ast::new(),
        diagnostics: Vec::new(),
        block_depth: 0,
        class_member_body_depth: 0,
    };
    p.parse_program();
    (p.ast, p.diagnostics)
}

pub(crate) struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    file: FileId,
    pos: usize,
    ast: Ast,
    diagnostics: Vec<Diagnostic>,
    /// Zero at top level; used to reject `import` inside nested blocks.
    block_depth: u32,
    /// Nonzero while parsing a class method or constructor body; gates `this`/`super`.
    class_member_body_depth: u32,
}

impl<'a> Parser<'a> {
    fn span(&self, start: u32, end: u32) -> Span {
        Span::new(self.file, start, end)
    }

    fn parse_program(&mut self) {
        while !self.is_at_eof() {
            if self.error_count() >= MAX_ERRORS {
                break;
            }
            if matches!(self.peek().kind, TokenKind::Semicolon) {
                self.advance();
                continue;
            }
            let pos_before = self.pos;
            if let Some(id) = self.parse_statement() {
                self.ast.top_level.push(id);
            } else {
                self.recover();
                if self.pos != pos_before && matches!(self.peek().kind, TokenKind::RightBrace) {
                    self.advance();
                    continue;
                }
                // Forward-progress guarantee: advance if recover didn't move.
                if self.pos == pos_before && !self.is_at_eof() {
                    self.advance();
                }
            }
        }
    }

    fn parse_statement(&mut self) -> Option<StmtId> {
        match self.peek().kind {
            TokenKind::Let => self.parse_let_or_const(false),
            TokenKind::Const => self.parse_let_or_const(true),
            TokenKind::Function => self.parse_function_decl(),
            TokenKind::Class => self.parse_class_decl(),
            TokenKind::Interface => self.parse_interface_decl(),
            TokenKind::Enum => self.parse_enum_decl(),
            TokenKind::Identifier if self.at_type_alias_head() => self.parse_type_alias_decl(),
            TokenKind::Export => self.parse_export(),
            TokenKind::Import => self.parse_import_decl(),
            TokenKind::If => self.parse_if(),
            TokenKind::While => self.parse_while(),
            TokenKind::Do => self.parse_do_while(),
            TokenKind::For => self.parse_for(),
            TokenKind::Switch => self.parse_switch(),
            TokenKind::Break => self.parse_break(),
            TokenKind::Continue => self.parse_continue(),
            TokenKind::Return => self.parse_return(),
            TokenKind::Throw => self.parse_throw(),
            TokenKind::Try => self.parse_try(),
            TokenKind::LeftBrace => self.parse_block(),
            TokenKind::Identifier if is_assign_lookahead(&self.peek_at(1).kind) => {
                self.parse_assign()
            }
            _ => self.parse_expression_statement(),
        }
    }

    fn parse_assign(&mut self) -> Option<StmtId> {
        let name_tok = self.advance();
        let target = self.ident_from_token(&name_tok);
        let op_tok = self.advance();
        let value = self.parse_expression()?;

        if !matches!(self.peek().kind, TokenKind::Semicolon) {
            self.error_at_peek("expected `;` after assignment");
            return None;
        }
        let semi = self.advance();

        let span = self.span(target.span.start, semi.span.end);
        let kind = if let Some(op) = compound_op_for_token(&op_tok.kind) {
            StmtKind::CompoundAssign {
                target,
                op,
                op_span: op_tok.span,
                value,
            }
        } else {
            StmtKind::Assign { target, value }
        };
        Some(self.ast.push_stmt(Stmt { kind, span }))
    }

    fn parse_expression_statement(&mut self) -> Option<StmtId> {
        let expr_id = self.parse_expression()?;
        let expr_span = self.ast.expr(expr_id).span;

        if is_assign_lookahead(&self.peek().kind) {
            return self.parse_assign_tail(expr_id, expr_span);
        }

        if !matches!(self.peek().kind, TokenKind::Semicolon) {
            self.error_at_peek("expected `;` after expression");
            return None;
        }
        let semi = self.advance();

        let span = self.span(expr_span.start, semi.span.end);
        Some(self.ast.push_stmt(Stmt {
            kind: StmtKind::Expr(expr_id),
            span,
        }))
    }

    fn parse_assign_tail(&mut self, lhs_id: ExprId, lhs_span: Span) -> Option<StmtId> {
        let op_tok = self.advance();
        let value_id = self.parse_expression()?;

        if !matches!(self.peek().kind, TokenKind::Semicolon) {
            self.error_at_peek("expected `;` after assignment");
            return None;
        }
        let semi = self.advance();
        let span = self.span(lhs_span.start, semi.span.end);
        let compound = compound_op_for_token(&op_tok.kind);

        let lhs_kind = self.ast.expr(lhs_id).kind.clone();
        match lhs_kind {
            ExprKind::FieldAccess { receiver, name } => {
                let kind = if let Some(op) = compound {
                    StmtKind::CompoundAssignField {
                        receiver,
                        field_name: name,
                        op,
                        op_span: op_tok.span,
                        value: value_id,
                    }
                } else {
                    StmtKind::AssignField {
                        receiver,
                        field_name: name,
                        value: value_id,
                    }
                };
                Some(self.ast.push_stmt(Stmt { kind, span }))
            }
            ExprKind::IndexAccess { receiver, index } => {
                let kind = if let Some(op) = compound {
                    StmtKind::CompoundAssignIndex {
                        receiver,
                        index,
                        op,
                        op_span: op_tok.span,
                        value: value_id,
                    }
                } else {
                    StmtKind::AssignIndex {
                        receiver,
                        index,
                        value: value_id,
                    }
                };
                Some(self.ast.push_stmt(Stmt { kind, span }))
            }
            _ => {
                self.error_at(lhs_span, "invalid assignment target");
                None
            }
        }
    }

    fn parse_let_or_const(&mut self, is_const: bool) -> Option<StmtId> {
        let doc = self.take_leading_doc();
        let kw = self.advance();

        let binding = match self.peek().kind {
            TokenKind::LeftBrace | TokenKind::LeftBracket => Some(self.parse_binding()?),
            _ => None,
        };

        let name = if binding.is_none() {
            let name_tok = self.expect_identifier("expected identifier after `let`/`const`")?;
            Some(self.ident_from_token(&name_tok))
        } else {
            None
        };

        let ty = if matches!(self.peek().kind, TokenKind::Colon) {
            self.advance();
            Some(self.parse_type_annotation()?)
        } else {
            None
        };

        if !matches!(self.peek().kind, TokenKind::Equals) {
            let (msg, help) = if is_const {
                (
                    "`const` declaration requires an initializer",
                    "const x: T = expr;".to_string(),
                )
            } else {
                (
                    "`let` declaration requires an initializer",
                    "let x: T = expr;".to_string(),
                )
            };
            self.error_at_peek_with_help(msg, vec![help]);
            return None;
        }
        self.advance();

        let value = self.parse_expression()?;

        if !matches!(self.peek().kind, TokenKind::Semicolon) {
            self.error_at_peek("expected `;` after declaration");
            return None;
        }
        let semi = self.advance();

        let span = self.span(kw.span.start, semi.span.end);
        let kind = match (binding, name) {
            (Some(binding), _) => {
                if is_const {
                    StmtKind::ConstPattern {
                        binding,
                        ty,
                        value,
                        doc,
                    }
                } else {
                    StmtKind::LetPattern {
                        binding,
                        ty,
                        value,
                        doc,
                    }
                }
            }
            (None, Some(name)) => {
                if is_const {
                    StmtKind::Const {
                        name,
                        ty,
                        value,
                        doc,
                    }
                } else {
                    StmtKind::Let {
                        name,
                        ty,
                        value,
                        doc,
                    }
                }
            }
            (None, None) => unreachable!("either binding or name must be Some"),
        };
        Some(self.ast.push_stmt(Stmt { kind, span }))
    }

    // One level only; nested patterns are rejected.
    fn parse_binding(&mut self) -> Option<Binding> {
        match self.peek().kind {
            TokenKind::LeftBrace => self.parse_object_binding(),
            TokenKind::LeftBracket => self.parse_array_binding(),
            _ => {
                self.error_at_peek("expected `{` or `[` to start a destructuring pattern");
                None
            }
        }
    }

    fn parse_object_binding(&mut self) -> Option<Binding> {
        let open = self.advance();
        let mut fields: Vec<ObjectPatternField> = Vec::new();
        let mut rest: Option<Ident> = None;

        if matches!(self.peek().kind, TokenKind::RightBrace) {
            self.error_at_peek("empty object destructuring pattern");
            return None;
        }

        loop {
            if matches!(self.peek().kind, TokenKind::DotDotDot) {
                let dots = self.advance();
                let name_tok = self.expect_identifier("expected identifier after `...`")?;
                let name = self.ident_from_token(&name_tok);
                if !matches!(self.peek().kind, TokenKind::RightBrace) {
                    self.error_at_peek_with_help(
                        "rest element must be the last element in a destructuring pattern",
                        vec!["move `...` after every other binding".to_string()],
                    );
                    return None;
                }
                rest = Some(Ident {
                    name: name.name,
                    span: self.span(dots.span.start, name.span.end),
                });
                break;
            }

            let source_tok = self.expect_property_name("expected field name in object pattern")?;
            let source = self.ident_from_token(&source_tok);
            let field_span_start = source.span.start;

            let (local, field_end) = if matches!(self.peek().kind, TokenKind::Colon) {
                self.advance();
                match self.peek().kind {
                    TokenKind::LeftBrace | TokenKind::LeftBracket => {
                        self.error_at_peek_with_help(
                                "nested destructuring is not supported",
                                vec![
                                    "destructure once and access nested fields explicitly: const { a } = obj; const inner = a.b;"
                                        .to_string(),
                                ],
                            );
                        return None;
                    }
                    _ => {
                        let local_tok =
                            self.expect_identifier("expected identifier after `:` in pattern")?;
                        let local = self.ident_from_token(&local_tok);
                        let end = local.span.end;
                        (local, end)
                    }
                }
            } else {
                if !matches!(source_tok.kind, TokenKind::Identifier) {
                    self.error_at_peek("expected `:` after keyword field name in object pattern");
                    return None;
                }
                let end = source.span.end;
                (source.clone(), end)
            };

            // no `undefined` in Submilli, so TS-style `{ a = 1 }` defaults don't translate.
            if matches!(self.peek().kind, TokenKind::Equals) {
                self.error_at_peek_with_help(
                    "default values inside destructuring patterns are not supported",
                    vec!["bind first, then use `??`: const a = obj.a ?? 1;".to_string()],
                );
                return None;
            }

            fields.push(ObjectPatternField {
                source,
                local,
                span: self.span(field_span_start, field_end),
            });

            match self.peek().kind {
                TokenKind::Comma => {
                    self.advance();
                    if matches!(self.peek().kind, TokenKind::RightBrace) {
                        break;
                    }
                }
                TokenKind::RightBrace => break,
                _ => {
                    self.error_at_peek("expected `,` or `}` in object pattern");
                    return None;
                }
            }
        }

        if !matches!(self.peek().kind, TokenKind::RightBrace) {
            self.error_at_peek("expected `}` to close object pattern");
            return None;
        }
        let close = self.advance();

        Some(Binding::Object {
            fields,
            rest,
            span: self.span(open.span.start, close.span.end),
        })
    }

    fn parse_array_binding(&mut self) -> Option<Binding> {
        let open = self.advance();
        let mut elems: Vec<Option<Ident>> = Vec::new();
        let mut rest: Option<Ident> = None;

        if matches!(self.peek().kind, TokenKind::RightBracket) {
            self.error_at_peek("empty array destructuring pattern");
            return None;
        }

        loop {
            match self.peek().kind {
                TokenKind::Comma => {
                    elems.push(None);
                    self.advance();
                    if matches!(self.peek().kind, TokenKind::RightBracket) {
                        break;
                    }
                    continue;
                }
                TokenKind::RightBracket => break,
                TokenKind::DotDotDot => {
                    let dots = self.advance();
                    let name_tok = self.expect_identifier("expected identifier after `...`")?;
                    let name = self.ident_from_token(&name_tok);
                    if !matches!(self.peek().kind, TokenKind::RightBracket) {
                        self.error_at_peek_with_help(
                            "rest element must be the last element in a destructuring pattern",
                            vec!["move `...` after every other binding".to_string()],
                        );
                        return None;
                    }
                    rest = Some(Ident {
                        name: name.name,
                        span: self.span(dots.span.start, name.span.end),
                    });
                    break;
                }
                TokenKind::LeftBrace | TokenKind::LeftBracket => {
                    self.error_at_peek_with_help(
                        "nested destructuring is not supported",
                        vec![
                            "destructure once and access nested elements explicitly: const [a] = arr; const inner = a[0];"
                                .to_string(),
                        ],
                    );
                    return None;
                }
                _ => {
                    let name_tok =
                        self.expect_identifier("expected identifier in array pattern")?;
                    let name = self.ident_from_token(&name_tok);

                    if matches!(self.peek().kind, TokenKind::Equals) {
                        self.error_at_peek_with_help(
                            "default values inside destructuring patterns are not supported",
                            vec!["bind first, then use `??`: const a = arr[0] ?? 1;".to_string()],
                        );
                        return None;
                    }
                    elems.push(Some(name));

                    match self.peek().kind {
                        TokenKind::Comma => {
                            self.advance();
                            if matches!(self.peek().kind, TokenKind::RightBracket) {
                                break;
                            }
                        }
                        TokenKind::RightBracket => break,
                        _ => {
                            self.error_at_peek("expected `,` or `]` in array pattern");
                            return None;
                        }
                    }
                }
            }
        }

        if !matches!(self.peek().kind, TokenKind::RightBracket) {
            self.error_at_peek("expected `]` to close array pattern");
            return None;
        }
        let close = self.advance();

        Some(Binding::Array {
            elems,
            rest,
            span: self.span(open.span.start, close.span.end),
        })
    }

    fn parse_function_decl(&mut self) -> Option<StmtId> {
        let doc = self.take_leading_doc();
        let kw = self.advance();

        let name_tok = self.expect_identifier("expected function name")?;
        let name = self.ident_from_token(&name_tok);

        let generics = if matches!(self.peek().kind, TokenKind::LessThan) {
            self.parse_generic_param_list()?
        } else {
            Vec::new()
        };

        if !matches!(self.peek().kind, TokenKind::LeftParen) {
            self.error_at_peek("expected `(` after function name");
            return None;
        }
        self.advance();

        let params = self.parse_param_list(false)?;
        self.advance(); // `)` — parse_param_list left us on it

        if !matches!(self.peek().kind, TokenKind::Colon) {
            self.error_at_peek_with_help(
                "expected `:` and return type",
                vec!["function name(): T { … }".to_string()],
            );
            return None;
        }
        self.advance();
        let (return_type, type_predicate) =
            self.parse_predicate_or_return_type(TypePos::Anywhere)?;

        let body = self.parse_block()?;
        let body_end = self.ast.stmt(body).span.end;

        Some(self.ast.push_stmt(Stmt {
            kind: StmtKind::Function {
                name,
                generics,
                params,
                return_type,
                type_predicate,
                body,
                doc,
            },
            span: self.span(kw.span.start, body_end),
        }))
    }

    fn parse_class_decl(&mut self) -> Option<StmtId> {
        let doc = self.take_leading_doc();
        let kw = self.advance();
        let name_tok = self.expect_identifier("expected class name")?;
        let name = self.ident_from_token(&name_tok);

        let generics = if matches!(self.peek().kind, TokenKind::LessThan) {
            self.parse_generic_param_list()?
        } else {
            Vec::new()
        };

        let extends = if matches!(self.peek().kind, TokenKind::Extends) {
            self.advance();
            Some(self.parse_type_annotation()?)
        } else {
            None
        };

        let mut implements: Vec<crate::TypeAnnotation> = Vec::new();
        if matches!(self.peek().kind, TokenKind::Implements) {
            self.advance();
            loop {
                implements.push(self.parse_type_annotation()?);
                if matches!(self.peek().kind, TokenKind::Comma) {
                    self.advance();
                    continue;
                }
                break;
            }
        }

        if !matches!(self.peek().kind, TokenKind::LeftBrace) {
            self.error_at_peek("expected `{` after class header");
            return None;
        }
        self.advance();

        let mut members: Vec<crate::ClassMember> = Vec::new();
        while !matches!(self.peek().kind, TokenKind::RightBrace | TokenKind::Eof) {
            // Stray `;` between members is accepted (TS allows it).
            if matches!(self.peek().kind, TokenKind::Semicolon) {
                self.advance();
                continue;
            }
            members.push(self.parse_class_member()?);
        }

        if !matches!(self.peek().kind, TokenKind::RightBrace) {
            self.error_at_peek("expected `}` to close class");
            return None;
        }
        let close = self.advance();

        Some(self.ast.push_stmt(Stmt {
            kind: StmtKind::ClassDecl {
                name,
                generics,
                extends,
                implements,
                members,
                doc,
            },
            span: self.span(kw.span.start, close.span.end),
        }))
    }

    fn parse_class_member(&mut self) -> Option<crate::ClassMember> {
        let doc = self.take_leading_doc();
        let member_start = self.peek().span.start;

        // Excluded modifiers — reject before anything else, naming the fix.
        if self.peek_identifier_text_is("protected") {
            let tok = self.advance();
            self.error_at_with_help(
                tok.span,
                "`protected` is not supported",
                vec!["use `private` or `public`".to_string()],
            );
            return None;
        }
        if self.peek_identifier_text_is("abstract") {
            let tok = self.advance();
            self.error_at_with_help(
                tok.span,
                "abstract classes are not supported",
                vec!["provide a concrete implementation instead of `abstract`".to_string()],
            );
            return None;
        }

        let modifiers = self.parse_class_modifiers();

        // Getter/setter: `get`/`set` <name> ( … ). A member literally named `get`/`set`
        // (followed directly by `(`) is an ordinary method and stays accepted.
        for (kw, kind) in [
            ("get", crate::AccessorKind::Get),
            ("set", crate::AccessorKind::Set),
        ] {
            if self.peek_identifier_text_is(kw)
                && is_property_name(&self.peek_at(1).kind)
                && matches!(self.peek_at(2).kind, TokenKind::LeftParen)
            {
                if let Some(span) = modifiers.static_span {
                    self.error_at_with_help(
                        span,
                        "static accessors are not supported",
                        vec!["use a static method: `static name(): T { … }`".to_string()],
                    );
                    return None;
                }
                return self.parse_accessor(member_start, modifiers, kind, doc);
            }
        }

        // Constructor.
        if self.peek_identifier_text_is("constructor")
            && matches!(self.peek_at(1).kind, TokenKind::LeftParen)
        {
            if let Some(span) = modifiers.static_span {
                self.error_at_with_help(
                    span,
                    "a constructor cannot be `static`",
                    vec![
                        "remove `static` — the constructor already belongs to the class, not \
                         instances"
                            .to_string(),
                    ],
                );
                return None;
            }
            return self.parse_constructor(member_start, doc);
        }

        let name = self.expect_property_ident("expected class member name")?;

        // Method.
        if matches!(self.peek().kind, TokenKind::LessThan | TokenKind::LeftParen) {
            let generics = if matches!(self.peek().kind, TokenKind::LessThan) {
                self.parse_generic_param_list()?
            } else {
                Vec::new()
            };
            if !matches!(self.peek().kind, TokenKind::LeftParen) {
                self.error_at_peek("expected `(` to start the method parameter list");
                return None;
            }
            self.advance();
            let params = self.parse_param_list(false)?;
            self.advance(); // `)` — parse_param_list left us on it
            if !matches!(self.peek().kind, TokenKind::Colon) {
                self.error_at_peek_with_help(
                    "expected `:` and return type",
                    vec!["method name(): T { … }".to_string()],
                );
                return None;
            }
            self.advance();
            let return_type = self.parse_type_annotation()?;
            let body = self.parse_class_member_body()?;
            let body_end = self.ast.stmt(body).span.end;
            return Some(crate::ClassMember::Method {
                name,
                modifiers,
                generics,
                params,
                return_type,
                body,
                span: self.span(member_start, body_end),
                doc,
            });
        }

        // Field.
        let optional = matches!(self.peek().kind, TokenKind::Question);
        if optional {
            let tok = self.advance();
            if modifiers.static_span.is_some() {
                self.error_at_with_help(
                    tok.span,
                    "a static field cannot be optional",
                    vec![
                        "a static field must be initialized; give it a value with `= …`"
                            .to_string(),
                    ],
                );
                return None;
            }
        }
        if !matches!(self.peek().kind, TokenKind::Colon) {
            if matches!(self.peek().kind, TokenKind::Equals) {
                self.error_at_peek_with_help(
                    "class fields require a type annotation",
                    vec!["add `: T` before the initializer, e.g. `name: string = …`".to_string()],
                );
            } else {
                self.error_at_peek("expected `:` and a type for the class field");
            }
            return None;
        }
        self.advance();
        let ty = self.parse_type_annotation()?;
        let initializer = if matches!(self.peek().kind, TokenKind::Equals) {
            self.advance();
            // A field initializer may read `this` (the typechecker binds it); gate
            // it like a member body so `this.x` parses.
            self.class_member_body_depth += 1;
            let init = self.parse_expression();
            self.class_member_body_depth -= 1;
            Some(init?)
        } else {
            None
        };
        if !matches!(self.peek().kind, TokenKind::Semicolon) {
            self.error_at_peek("expected `;` after class field");
            return None;
        }
        let semi = self.advance();
        Some(crate::ClassMember::Field {
            name,
            modifiers,
            optional,
            ty,
            initializer,
            span: self.span(member_start, semi.span.end),
            doc,
        })
    }

    fn parse_constructor(
        &mut self,
        member_start: u32,
        doc: Option<crate::DocComment>,
    ) -> Option<crate::ClassMember> {
        self.advance(); // `constructor`
        self.advance(); // `(` — guaranteed by the caller
        let params = self.parse_param_list(true)?;
        self.advance(); // `)` — parse_param_list left us on it
        if matches!(self.peek().kind, TokenKind::Colon) {
            let colon = self.advance();
            let _ = self.parse_type_annotation();
            self.error_at_with_help(
                colon.span,
                "a constructor cannot declare a return type",
                vec![
                    "drop the `: T` — a constructor always returns the class instance".to_string(),
                ],
            );
        }
        let body = self.parse_class_member_body()?;
        let body_end = self.ast.stmt(body).span.end;
        Some(crate::ClassMember::Constructor {
            params,
            body,
            span: self.span(member_start, body_end),
            doc,
        })
    }

    /// `get name(): T { … }` / `set name(v: T) { … }`. The caller has confirmed the
    /// `get`/`set` keyword, a property name, and a `(` follow.
    fn parse_accessor(
        &mut self,
        member_start: u32,
        modifiers: crate::ClassModifiers,
        kind: crate::AccessorKind,
        doc: Option<crate::DocComment>,
    ) -> Option<crate::ClassMember> {
        self.advance(); // `get` / `set`
        let name = self.expect_property_ident("expected accessor name")?;
        self.advance(); // `(` — guaranteed by the caller
        let mut params = self.parse_param_list(false)?;
        self.advance(); // `)`
        let param = match kind {
            crate::AccessorKind::Get => {
                if !params.is_empty() {
                    self.error_at(name.span, "a getter cannot declare parameters");
                }
                None
            }
            crate::AccessorKind::Set => {
                if params.len() != 1 {
                    self.error_at(name.span, "a setter must declare exactly one parameter");
                    return None;
                }
                Some(Box::new(params.remove(0)))
            }
        };
        let return_type = if matches!(self.peek().kind, TokenKind::Colon) {
            self.advance();
            Some(self.parse_type_annotation()?)
        } else {
            None
        };
        let body = self.parse_class_member_body()?;
        let body_end = self.ast.stmt(body).span.end;
        Some(crate::ClassMember::Accessor {
            name,
            modifiers,
            kind,
            param,
            return_type,
            body,
            span: self.span(member_start, body_end),
            doc,
        })
    }

    /// `public`/`private`/`static`/`readonly`. Duplicate or conflicting modifiers are
    /// diagnosed but parsing continues so the member shape is still recovered.
    /// Visibility must precede `static` (the TypeScript order); `readonly` is accepted
    /// on either side of `static` — no semantic difference under our subset.
    fn parse_class_modifiers(&mut self) -> crate::ClassModifiers {
        let mut visibility = crate::Visibility::Public;
        let mut visibility_span: Option<Span> = None;
        let mut readonly: Option<Span> = None;
        let mut static_span: Option<Span> = None;
        loop {
            if self.peek_word_is_class_modifier("public")
                || self.peek_word_is_class_modifier("private")
            {
                let is_private = self.peek_identifier_text_is("private");
                let tok = self.advance();
                if visibility_span.is_some() {
                    self.error_at_with_help(
                        tok.span,
                        "a class member may have at most one visibility modifier",
                        vec!["keep a single `public` or `private`".to_string()],
                    );
                } else {
                    if static_span.is_some() {
                        let word = if is_private { "private" } else { "public" };
                        self.error_at_with_help(
                            tok.span,
                            format!("`{word}` must come before `static`"),
                            vec![format!("write `{word} static <name>`")],
                        );
                    }
                    visibility = if is_private {
                        crate::Visibility::Private
                    } else {
                        crate::Visibility::Public
                    };
                    visibility_span = Some(tok.span);
                }
                continue;
            }
            if self.peek_word_is_class_modifier("static") {
                let tok = self.advance();
                if static_span.is_some() {
                    self.error_at(tok.span, "duplicate `static` modifier");
                } else {
                    static_span = Some(tok.span);
                }
                continue;
            }
            if self.peek_word_is_class_modifier("readonly") {
                let tok = self.advance();
                if readonly.is_some() {
                    self.error_at(tok.span, "duplicate `readonly` modifier");
                } else {
                    readonly = Some(tok.span);
                }
                continue;
            }
            break;
        }
        crate::ClassModifiers {
            visibility,
            visibility_span,
            readonly,
            static_span,
        }
    }

    /// A contextual modifier keyword counts as a modifier only when another member token
    /// (the real name, or a further modifier) follows — otherwise the word is the member
    /// name itself (e.g. a field named `private`). Mirrors `eat_readonly_property_modifier`.
    fn peek_word_is_class_modifier(&self, word: &str) -> bool {
        if !self.peek_identifier_text_is(word) {
            return false;
        }
        let next = &self.peek_at(1).kind;
        is_property_name(next) || matches!(next, TokenKind::StringLiteral(_))
    }

    fn parse_class_member_body(&mut self) -> Option<StmtId> {
        self.class_member_body_depth += 1;
        let body = self.parse_block();
        self.class_member_body_depth -= 1;
        body
    }

    fn parse_interface_decl(&mut self) -> Option<StmtId> {
        let doc = self.take_leading_doc();
        let kw = self.advance();
        let name_tok = self.expect_identifier("expected interface name")?;
        let name = self.ident_from_token(&name_tok);

        let generics = if matches!(self.peek().kind, TokenKind::LessThan) {
            self.parse_generic_param_list()?
        } else {
            Vec::new()
        };

        if !matches!(self.peek().kind, TokenKind::LeftBrace) {
            self.error_at_peek("expected `{` after interface name");
            return None;
        }
        self.advance();

        let mut members: Vec<crate::InterfaceMember> = Vec::new();
        while !matches!(self.peek().kind, TokenKind::RightBrace | TokenKind::Eof) {
            // Call signatures `(params): ret;` are stored under the sentinel name `@call`.
            // The `@` prefix is not a valid identifier start, so collisions with user methods
            // are impossible.
            let member_doc = self.take_leading_doc();
            if matches!(self.peek().kind, TokenKind::LeftParen) {
                let open = self.advance();
                let params = self.parse_param_list(false)?;
                self.advance();
                if !matches!(self.peek().kind, TokenKind::Colon) {
                    self.error_at_peek("expected `:` and return type");
                    return None;
                }
                self.advance();
                let return_type = self.parse_type_annotation()?;
                let end = self.finish_interface_member(return_type.span.end)?;
                let span = self.span(open.span.start, end);
                let sentinel = Ident {
                    name: "@call".to_string(),
                    span: open.span,
                };
                members.push(crate::InterfaceMember::Method {
                    name: sentinel,
                    generics: Vec::new(),
                    params,
                    return_type,
                    span,
                    doc: member_doc,
                });
                continue;
            }
            if matches!(self.peek().kind, TokenKind::LessThan) {
                self.error_at_peek("generic call signatures are not yet supported");
                return None;
            }
            let readonly = self.eat_readonly_property_modifier();
            let member_name = self.expect_property_ident("expected interface member name")?;
            let optional = matches!(self.peek().kind, TokenKind::Question);
            if optional {
                self.advance();
                if matches!(self.peek().kind, TokenKind::LeftParen | TokenKind::LessThan) {
                    self.error_at_peek("optional interface methods are not supported");
                    return None;
                }
            }
            if matches!(self.peek().kind, TokenKind::Colon) {
                self.advance();
                let ty = self.parse_type_annotation()?;
                let end = self.finish_interface_member(ty.span.end)?;
                let span = self.span(member_name.span.start, end);
                members.push(crate::InterfaceMember::Property {
                    name: member_name,
                    ty,
                    optional,
                    readonly,
                    span,
                    doc: member_doc,
                });
                continue;
            }
            let m_generics = if matches!(self.peek().kind, TokenKind::LessThan) {
                self.parse_generic_param_list()?
            } else {
                Vec::new()
            };
            if !matches!(self.peek().kind, TokenKind::LeftParen) {
                self.error_at_peek(
                    "expected `(` to start a method signature or `:` to start a property",
                );
                return None;
            }
            self.advance();
            let params = self.parse_param_list(false)?;
            self.advance();
            if !matches!(self.peek().kind, TokenKind::Colon) {
                self.error_at_peek("expected `:` and return type");
                return None;
            }
            self.advance();
            let return_type = self.parse_type_annotation()?;
            let end = self.finish_interface_member(return_type.span.end)?;
            let span = self.span(member_name.span.start, end);
            members.push(crate::InterfaceMember::Method {
                name: member_name,
                generics: m_generics,
                params,
                return_type,
                span,
                doc: member_doc,
            });
        }

        if !matches!(self.peek().kind, TokenKind::RightBrace) {
            self.error_at_peek("expected `}` to close interface");
            return None;
        }
        let close = self.advance();

        Some(self.ast.push_stmt(Stmt {
            kind: StmtKind::InterfaceDecl {
                name,
                generics,
                members,
                doc,
            },
            span: self.span(kw.span.start, close.span.end),
        }))
    }

    /// Consume the separator after an interface member and return where the member
    /// ends. `;` and `,` are interchangeable and the last member's is optional — the
    /// rule `parse_object_type_annotation` already applies to inline type literals,
    /// extended here to `interface` bodies. `body_end` is where the member ends when
    /// it carries no separator of its own.
    fn finish_interface_member(&mut self, body_end: u32) -> Option<u32> {
        match self.peek().kind {
            TokenKind::Semicolon | TokenKind::Comma => Some(self.advance().span.end),
            TokenKind::RightBrace => Some(body_end),
            _ => {
                self.error_at_peek("expected `;`, `,`, or `}` after interface member");
                None
            }
        }
    }

    fn parse_type_alias_decl(&mut self) -> Option<StmtId> {
        let doc = self.take_leading_doc();
        let kw = self.advance();
        let name_tok = self.expect_identifier("expected type alias name")?;
        let name = self.ident_from_token(&name_tok);

        let generics: Vec<Ident> = if matches!(self.peek().kind, TokenKind::LessThan) {
            self.parse_generic_param_list()?
        } else {
            Vec::new()
        };

        if !matches!(self.peek().kind, TokenKind::Equals) {
            self.error_at_peek("expected `=` after type alias name");
            return None;
        }
        self.advance();

        let ty = self.parse_type_annotation()?;

        if !matches!(self.peek().kind, TokenKind::Semicolon) {
            self.error_at_peek("expected `;` after type alias body");
            return None;
        }
        let semi = self.advance();

        Some(self.ast.push_stmt(Stmt {
            kind: StmtKind::TypeAliasDecl {
                name,
                generics,
                ty,
                doc,
            },
            span: self.span(kw.span.start, semi.span.end),
        }))
    }

    // Parser enforces kind-coherence: an enum cannot mix numeric and string initializers.
    fn parse_enum_decl(&mut self) -> Option<StmtId> {
        #[derive(Copy, Clone)]
        enum Kind {
            Number,
            String,
        }
        let doc = self.take_leading_doc();
        let kw = self.advance();
        let name_tok = self.expect_identifier("expected enum name")?;
        let name = self.ident_from_token(&name_tok);

        if !matches!(self.peek().kind, TokenKind::LeftBrace) {
            self.error_at_peek("expected `{` after enum name");
            return None;
        }
        self.advance();

        let mut members: Vec<EnumMember> = Vec::new();
        let mut first_explicit: Option<(Kind, Span)> = None;
        while !matches!(self.peek().kind, TokenKind::RightBrace | TokenKind::Eof) {
            // ASI inserts `;` after `,` on a newline — tolerate it so multi-line enums parse.
            if matches!(self.peek().kind, TokenKind::Semicolon) {
                self.advance();
                continue;
            }
            let member_doc = self.take_leading_doc();
            let member_name_tok = self.expect_identifier("expected enum member name")?;
            let member_name = self.ident_from_token(&member_name_tok);

            let value = if matches!(self.peek().kind, TokenKind::Equals) {
                self.advance();
                let init = self.parse_enum_initializer()?;
                let (this_kind, this_span) = match &init {
                    EnumInitializer::Number { span, .. } => (Kind::Number, *span),
                    EnumInitializer::String { span, .. } => (Kind::String, *span),
                };
                match first_explicit {
                    None => first_explicit = Some((this_kind, this_span)),
                    Some((prev, _)) => {
                        let mismatched = matches!(
                            (prev, this_kind),
                            (Kind::Number, Kind::String) | (Kind::String, Kind::Number)
                        );
                        if mismatched {
                            self.error_at(
                                this_span,
                                "mixed numeric and string enum members are not allowed",
                            );
                        }
                    }
                }
                Some(init)
            } else {
                None
            };

            let end_span = value
                .as_ref()
                .map_or(member_name.span, super::ast::EnumInitializer::span);
            let member_span = self.span(member_name.span.start, end_span.end);
            members.push(EnumMember {
                name: member_name,
                value,
                span: member_span,
                doc: member_doc,
            });

            match self.peek().kind {
                // ASI supplies the `;` before a closing `}` and after a newline, so an
                // enum body sees one wherever a comma was omitted.
                TokenKind::Comma | TokenKind::Semicolon => {
                    self.advance();
                }
                TokenKind::RightBrace | TokenKind::Eof => {}
                _ => {
                    self.error_at_peek("expected `,` or `}` after enum member");
                    return None;
                }
            }
        }

        if !matches!(self.peek().kind, TokenKind::RightBrace) {
            self.error_at_peek("expected `}` to close enum");
            return None;
        }
        let close = self.advance();

        Some(self.ast.push_stmt(Stmt {
            kind: StmtKind::EnumDecl { name, members, doc },
            span: self.span(kw.span.start, close.span.end),
        }))
    }

    /// `export` on a top-level declaration. Two forms (multi-file.md §3):
    /// Form 1 marks a declaration (`export function …`) — the declaration is
    /// parsed normally and recorded in `exported_decls`; Form 2 is a re-export
    /// (`export { a, b as c } from "./util";` / `export { x };`) parsed into an
    /// `ExportFrom` node. `export` is top-level only; inside a function body it
    /// is a parse error. `export default` stays rejected.
    fn parse_export(&mut self) -> Option<StmtId> {
        let export_tok = self.advance();
        let export_span = export_tok.span;

        let nested = self.block_depth > 0;
        if nested {
            self.error_at_with_help(
                export_span,
                "`export` statements must appear at the top of the file",
                vec!["move this `export` to the top level of the module".to_string()],
            );
            // Fall through to parse the declaration / list anyway so recovery
            // stays in sync, but return None below so it isn't recorded.
        }

        // `export type { … }` — TS's type-only re-export, accepted as a no-op
        // synonym: exports carry both value and type spaces here.
        if self.peek_identifier_text_is("type")
            && matches!(self.peek_at(1).kind, TokenKind::LeftBrace)
        {
            self.advance();
        }

        let declares = matches!(
            self.peek().kind,
            TokenKind::Function
                | TokenKind::Class
                | TokenKind::Const
                | TokenKind::Let
                | TokenKind::Interface
                | TokenKind::Enum
        ) || self.at_type_alias_head();
        if declares {
            // Carry any doc comment that attached to `export` onto the
            // declaration token so the declaration parser still sees it.
            if export_tok.leading_doc.is_some() && self.peek().leading_doc.is_none() {
                self.tokens[self.pos].leading_doc = export_tok.leading_doc;
            }
            let id = self.parse_statement()?;
            if nested {
                return None;
            }
            self.ast.exported_decls.push(ExportedDecl {
                stmt: id,
                export_span,
            });
            return Some(id);
        }

        match self.peek().kind {
            TokenKind::LeftBrace => {
                // A doc comment attaches to the `export` keyword, which we've
                // already consumed — carry it onto the re-export node.
                let doc = export_tok
                    .leading_doc
                    .as_ref()
                    .map(crate::parse_doc_comment)
                    .or_else(|| self.take_leading_doc());
                self.parse_export_from(export_span, nested, doc)
            }
            TokenKind::Default => {
                self.error_at_peek_with_help(
                    "`export default` is not supported",
                    vec![
                        "default exports are out of scope; drop `default` — a named \
                         `export` marks the declaration visible to sibling modules"
                            .to_string(),
                    ],
                );
                None
            }
            _ => {
                self.error_at_peek_with_help(
                    "expected a declaration after `export`",
                    vec![
                        "`export` may precede `function`, `class`, `const`, `let`, `type`, \
                         `interface`, or `enum`, or take the form `export { … }`"
                            .to_string(),
                    ],
                );
                None
            }
        }
    }

    /// Parse Form 2 re-export: `export { a, b as c } from "./util";` or the
    /// bare `export { x };`. The leading `export` has already been consumed.
    fn parse_export_from(
        &mut self,
        export_span: Span,
        nested: bool,
        doc: Option<crate::DocComment>,
    ) -> Option<StmtId> {
        let (specs, open_span) = self.parse_specifier_list("export")?;
        if specs.is_empty() {
            self.error_at_with_help(
                open_span,
                "empty export specifier list",
                vec!["name what you re-export: `export { name } from \"./util\";`".to_string()],
            );
            return None;
        }

        let source = if self.peek_identifier_text_is("from") {
            self.advance();
            let module_tok = self.peek().clone();
            let module = if let TokenKind::StringLiteral(s) = &module_tok.kind {
                s.clone()
            } else {
                self.error_at_peek("expected module specifier string after `from`");
                return None;
            };
            self.advance();
            Some((module, module_tok.span))
        } else {
            None
        };

        if !matches!(self.peek().kind, TokenKind::Semicolon) {
            self.error_at_peek("expected `;` after export statement");
            return None;
        }
        let semi = self.advance();

        if nested {
            return None;
        }

        Some(self.ast.push_stmt(Stmt {
            kind: StmtKind::ExportFrom { specs, source, doc },
            span: self.span(export_span.start, semi.span.end),
        }))
    }

    fn parse_import_decl(&mut self) -> Option<StmtId> {
        let doc = self.take_leading_doc();
        let kw = self.advance();
        let kw_span = kw.span;

        let nested = self.block_depth > 0;
        if nested {
            self.error_at_with_help(
                kw_span,
                "`import` statements must appear at the top of the file",
                vec!["move this `import` outside any block / function body".to_string()],
            );
            // Continue parsing to consume the whole import so recovery isn't confused.
        }

        let kind = self.parse_import_kind()?;

        if !self.peek_identifier_text_is("from") {
            self.error_at_peek("expected `from` after import specifier list");
            return None;
        }
        self.advance();

        let module_tok = self.peek().clone();
        let module = if let TokenKind::StringLiteral(s) = &module_tok.kind {
            s.clone()
        } else {
            self.error_at_peek("expected module specifier string after `from`");
            return None;
        };
        let module_span = module_tok.span;
        self.advance();

        if !matches!(self.peek().kind, TokenKind::Semicolon) {
            self.error_at_peek("expected `;` after import statement");
            return None;
        }
        let semi = self.advance();

        if nested {
            return None;
        }

        Some(self.ast.push_stmt(Stmt {
            kind: StmtKind::Import {
                module,
                module_span,
                kind,
                doc,
            },
            span: self.span(kw_span.start, semi.span.end),
        }))
    }

    fn parse_import_kind(&mut self) -> Option<ImportKind> {
        // `import type …` — TS's type-only import, accepted as a no-op synonym:
        // imports carry both value and type spaces here. `import type from "m"`
        // stays a namespace import named `type`, so `type` is a modifier only
        // when what follows can't be a complete import clause without it.
        if self.peek_identifier_text_is("type")
            && match self.peek_at(1).kind {
                TokenKind::LeftBrace | TokenKind::Star => true,
                TokenKind::Identifier => {
                    !(self.peek_at_is_word(1, "from")
                        && matches!(self.peek_at(2).kind, TokenKind::StringLiteral(_)))
                }
                _ => false,
            }
        {
            self.advance();
        }

        match self.peek().kind {
            TokenKind::LeftBrace => self.parse_named_imports(),
            TokenKind::Identifier => {
                let local_tok = self.advance();
                let local_name = self.ident_from_token(&local_tok);
                if matches!(self.peek().kind, TokenKind::Comma) {
                    self.error_at_peek_with_help(
                        "combining default and named imports is not supported",
                        vec!["use two separate `import` statements".to_string()],
                    );
                    return None;
                }
                Some(ImportKind::Namespace { local_name })
            }
            // `import * as ns from "pkg"` — forgiveness synonym for `import ns from "pkg"`.
            // Both are identical since we have no default exports.
            TokenKind::Star => {
                self.advance();
                if !self.peek_identifier_text_is("as") {
                    self.error_at_peek_with_help(
                        "expected `as <name>` after `import *`",
                        vec![
                            "name the namespace binding: \
                             `import * as ns from \"pkg\";` (or use the \
                             short form: `import ns from \"pkg\";`)"
                                .to_string(),
                        ],
                    );
                    return None;
                }
                self.advance();
                let local_tok = self.expect_identifier("expected namespace name after `as`")?;
                let local_name = self.ident_from_token(&local_tok);
                Some(ImportKind::Namespace { local_name })
            }
            TokenKind::StringLiteral(_) => {
                self.error_at_peek_with_help(
                    "side-effect-only imports are not supported",
                    vec![
                        "name what you import: `import { name } from \"pkg\";` \
                         or `import ns from \"pkg\";`"
                            .to_string(),
                    ],
                );
                None
            }
            _ => {
                self.error_at_peek("expected `{` or namespace name after `import`");
                None
            }
        }
    }

    fn parse_named_imports(&mut self) -> Option<ImportKind> {
        let (specs, open_span) = self.parse_specifier_list("import")?;
        if specs.is_empty() {
            self.error_at_with_help(
                open_span,
                "empty import specifier list",
                vec!["name what you import: `import { name } from \"pkg\";`".to_string()],
            );
            return None;
        }
        Some(ImportKind::Named(specs))
    }

    /// Parse a `{ a, b as c }` specifier list shared by `import` and re-`export`.
    /// `what` names the construct in diagnostics. Returns the specifiers (possibly
    /// empty — callers reject empty with construct-specific help) and the span of
    /// the opening brace.
    fn parse_specifier_list(&mut self, what: &str) -> Option<(Vec<ImportSpecifier>, Span)> {
        let open = self.advance();
        let mut specs: Vec<ImportSpecifier> = Vec::new();
        while !matches!(self.peek().kind, TokenKind::RightBrace | TokenKind::Eof) {
            // Inline type-only modifier (`{ type X }`), accepted as a no-op
            // synonym. `{ type as t }` instead names the binding `type`.
            if self.peek_identifier_text_is("type")
                && matches!(self.peek_at(1).kind, TokenKind::Identifier)
                && !self.peek_at_is_word(1, "as")
            {
                self.advance();
            }
            let imported_tok =
                self.expect_identifier(&format!("expected {what} specifier name"))?;
            let imported_name = self.ident_from_token(&imported_tok);

            let local_name = if self.peek_identifier_text_is("as") {
                self.advance();
                let local_tok = self.expect_identifier("expected local name after `as`")?;
                self.ident_from_token(&local_tok)
            } else {
                imported_name.clone()
            };

            specs.push(ImportSpecifier {
                imported_name,
                local_name,
            });

            match self.peek().kind {
                TokenKind::Comma => {
                    self.advance();
                }
                TokenKind::RightBrace | TokenKind::Eof => {}
                _ => {
                    self.error_at_peek(format!("expected `,` or `}}` after {what} specifier"));
                    return None;
                }
            }
        }

        if !matches!(self.peek().kind, TokenKind::RightBrace) {
            self.error_at_peek(format!("expected `}}` to close {what} specifier list"));
            return None;
        }
        self.advance();

        Some((specs, open.span))
    }

    fn parse_enum_initializer(&mut self) -> Option<EnumInitializer> {
        let neg = if matches!(self.peek().kind, TokenKind::Minus) {
            Some(self.advance())
        } else {
            None
        };
        let tok = self.advance();
        match &tok.kind {
            TokenKind::NumberLiteral(n) => {
                let value = if neg.is_some() { -n } else { *n };
                let start = neg.as_ref().map_or(tok.span.start, |t| t.span.start);
                Some(EnumInitializer::Number {
                    value,
                    span: self.span(start, tok.span.end),
                })
            }
            TokenKind::StringLiteral(s) => {
                if let Some(neg_tok) = &neg {
                    self.error_at(
                        self.span(neg_tok.span.start, tok.span.end),
                        "cannot negate a string enum initializer",
                    );
                    return None;
                }
                Some(EnumInitializer::String {
                    value: s.clone(),
                    span: tok.span,
                })
            }
            _ => {
                let span = neg.map_or(tok.span, |t| self.span(t.span.start, tok.span.end));
                self.error_at(
                    span,
                    "expected a number or string literal for enum initializer",
                );
                None
            }
        }
    }

    fn parse_type_argument_list(&mut self) -> Option<(Vec<TypeAnnotation>, u32)> {
        let lt = self.advance();
        if matches!(self.peek().kind, TokenKind::GreaterThan) {
            self.error_at(lt.span, "empty generic argument list `<>` is not allowed");
            return None;
        }
        let mut args = Vec::new();
        loop {
            args.push(self.parse_type_annotation()?);
            match self.peek().kind {
                TokenKind::Comma => {
                    self.advance();
                    if matches!(self.peek().kind, TokenKind::GreaterThan) {
                        let gt = self.advance();
                        return Some((args, gt.span.end));
                    }
                }
                TokenKind::GreaterThan => {
                    let gt = self.advance();
                    return Some((args, gt.span.end));
                }
                _ => {
                    self.error_at(
                        lt.span,
                        "expected `,` or `>` to close generic argument list",
                    );
                    return None;
                }
            }
        }
    }

    fn parse_generic_param_list(&mut self) -> Option<Vec<Ident>> {
        let lt = self.advance();
        if matches!(self.peek().kind, TokenKind::GreaterThan) {
            self.error_at(lt.span, "empty generic parameter list `<>` is not allowed");
            return None;
        }
        let mut generics = Vec::new();
        loop {
            let name_tok = self.expect_identifier("expected generic parameter name")?;
            generics.push(self.ident_from_token(&name_tok));
            match self.peek().kind {
                TokenKind::Comma => {
                    self.advance();
                    if matches!(self.peek().kind, TokenKind::GreaterThan) {
                        self.advance();
                        return Some(generics);
                    }
                }
                TokenKind::GreaterThan => {
                    self.advance();
                    return Some(generics);
                }
                _ => {
                    self.error_at_peek("expected `,` or `>`");
                    return None;
                }
            }
        }
    }

    fn parse_param_list(&mut self, allow_param_properties: bool) -> Option<Vec<ParamDecl>> {
        if matches!(self.peek().kind, TokenKind::RightParen) {
            return Some(Vec::new());
        }
        let mut params = Vec::new();
        loop {
            let param = self.parse_param(allow_param_properties)?;
            let is_rest = param.rest;
            params.push(param);
            match self.peek().kind {
                TokenKind::Comma => {
                    if is_rest {
                        let comma = self.advance();
                        self.error_at(comma.span, "rest parameter must be the last parameter");
                        return None;
                    }
                    self.advance();
                    if matches!(self.peek().kind, TokenKind::RightParen) {
                        break;
                    }
                }
                TokenKind::RightParen => break,
                _ => {
                    self.error_at_peek("expected `,` or `)`");
                    return None;
                }
            }
        }
        let first_default = params.iter().position(|p| p.default.is_some());
        if let Some(first) = first_default {
            for p in &params[first + 1..] {
                if p.default.is_none() && !p.rest {
                    // Pattern params have an empty `name` pre-lowering; anchor at the pattern span.
                    let (span, label) = match &p.pattern {
                        Some(b) => (b.span(), "destructured parameter".to_string()),
                        None => (p.name.span, format!("parameter `{}`", p.name.name)),
                    };
                    self.error_at(
                        span,
                        format!("required {label} cannot follow a parameter with a default value",),
                    );
                    return None;
                }
            }
        }
        Some(params)
    }

    fn parse_param(&mut self, allow_param_properties: bool) -> Option<ParamDecl> {
        // Parameter properties: `constructor(public x: T)` declares + assigns a
        // field. A modifier keyword counts only when a parameter name follows it
        // (so a param literally named `public`/`readonly` still parses).
        let modifiers = if self.peek_word_is_class_modifier("public")
            || self.peek_word_is_class_modifier("private")
            || self.peek_word_is_class_modifier("readonly")
        {
            let span = self.peek().span;
            let parsed = self.parse_class_modifiers();
            if allow_param_properties {
                Some(parsed)
            } else {
                self.error_at_with_help(
                    span,
                    "parameter properties are only allowed in a constructor",
                    vec![
                        "move the `public`/`private`/`readonly` modifier to a constructor parameter"
                            .to_string(),
                    ],
                );
                None
            }
        } else {
            None
        };

        let rest = if matches!(self.peek().kind, TokenKind::DotDotDot) {
            self.advance();
            true
        } else {
            false
        };
        if modifiers.is_some() && rest {
            self.error_at_peek("a parameter property cannot be a rest parameter");
            return None;
        }

        // Placeholder `name` is the empty identifier at the pattern's span;
        // the pre-infer lowering pass replaces it with a fresh `#pattern_p_N`.
        let (name, pattern) = match self.peek().kind {
            TokenKind::LeftBrace | TokenKind::LeftBracket => {
                if rest {
                    self.error_at_peek("rest parameter cannot be destructured");
                    return None;
                }
                if modifiers.is_some() {
                    self.error_at_peek("a parameter property cannot be destructured");
                    return None;
                }
                let binding = self.parse_binding()?;
                let placeholder = Ident {
                    name: String::new(),
                    span: binding.span(),
                };
                (placeholder, Some(binding))
            }
            _ => {
                let name_tok = self.expect_identifier(if rest {
                    "expected identifier after `...`"
                } else {
                    "expected parameter name"
                })?;
                let name = self.ident_from_token(&name_tok);
                (name, None)
            }
        };

        if matches!(self.peek().kind, TokenKind::Question) {
            self.error_at_peek("optional function parameters are not yet supported");
            return None;
        }
        if !matches!(self.peek().kind, TokenKind::Colon) {
            if rest {
                self.error_at_peek_with_help(
                    "rest parameter requires a type annotation",
                    vec!["function name(...args: T[]): R { … }".to_string()],
                );
            } else {
                self.error_at_peek_with_help(
                    "parameter requires a type annotation",
                    vec!["function name(x: T, y: U): R { … }".to_string()],
                );
            }
            return None;
        }
        self.advance();
        let ty = self.parse_type_annotation()?;
        let default = if matches!(self.peek().kind, TokenKind::Equals) {
            // Rest's default is the empty array — no explicit default allowed.
            if rest {
                self.error_at_peek_with_help(
                    "rest parameter cannot have a default value",
                    vec!["omit the `= …`; an unspecified rest defaults to `[]`".to_string()],
                );
                return None;
            }
            // The lowering pass materialises a synthetic identifier for pattern params first;
            // defaults can't compose with that before lowering.
            if pattern.is_some() {
                self.error_at_peek_with_help(
                    "default values are not supported on destructured parameters",
                    vec![
                        "pass a defaulted whole object at the call site, or use `??` per binding inside the body"
                            .to_string(),
                    ],
                );
                return None;
            }
            self.advance();
            Some(self.parse_expression()?)
        } else {
            None
        };
        Some(ParamDecl {
            name,
            ty: Some(ty),
            default,
            pattern,
            rest,
            modifiers,
        })
    }

    /// Drop a `;` that ASI inserted directly before `next`, where the grammar requires
    /// `next` and so no statement can have ended. ASI is a token-stream pre-pass with no
    /// parser feedback, so it cannot see that a `}` ends a `do` body rather than a
    /// statement, or that the `{` after a signature's return annotation opens its body.
    fn eat_asi_semicolon_before(&mut self, next: TokenKind) {
        if matches!(self.peek().kind, TokenKind::Semicolon) && self.peek_at(1).kind == next {
            self.advance();
        }
    }

    fn parse_block(&mut self) -> Option<StmtId> {
        self.eat_asi_semicolon_before(TokenKind::LeftBrace);
        if !matches!(self.peek().kind, TokenKind::LeftBrace) {
            self.error_at_peek("expected `{`");
            return None;
        }
        let open = self.advance();
        self.block_depth += 1;

        let mut stmts = Vec::new();
        while !matches!(self.peek().kind, TokenKind::RightBrace | TokenKind::Eof) {
            if matches!(self.peek().kind, TokenKind::Semicolon) {
                self.advance();
                continue;
            }
            let pos_before = self.pos;
            if let Some(id) = self.parse_statement() {
                stmts.push(id);
            } else {
                self.recover();
                if self.pos == pos_before && !self.is_at_eof() {
                    self.advance();
                }
            }
        }

        self.block_depth -= 1;
        if !matches!(self.peek().kind, TokenKind::RightBrace) {
            self.error_at_peek("expected `}`");
            return None;
        }
        let close = self.advance();

        let span = self.span(open.span.start, close.span.end);
        Some(self.ast.push_stmt(Stmt {
            kind: StmtKind::Block(stmts),
            span,
        }))
    }

    // Control-flow bodies accept either a braced block or a single statement;
    // `if (c) return x;` is identical to `if (c) { return x; }` under our type
    // system (forgiveness principle). Downstream passes assume a `Block`, so a
    // braceless statement is wrapped in one.
    fn parse_control_body(&mut self) -> Option<StmtId> {
        if matches!(self.peek().kind, TokenKind::LeftBrace) {
            return self.parse_block();
        }
        self.block_depth += 1;
        let stmt = self.parse_statement();
        self.block_depth -= 1;
        let stmt = stmt?;
        let span = self.ast.stmt(stmt).span;
        Some(self.ast.push_stmt(Stmt {
            kind: StmtKind::Block(vec![stmt]),
            span,
        }))
    }

    fn parse_if(&mut self) -> Option<StmtId> {
        let kw = self.advance();
        let condition = self.parse_paren_condition()?;
        let then_block = self.parse_control_body()?;

        let else_block = if matches!(self.peek().kind, TokenKind::Else) {
            self.advance();
            if matches!(self.peek().kind, TokenKind::If) {
                Some(self.parse_if()?)
            } else {
                Some(self.parse_control_body()?)
            }
        } else {
            None
        };

        let end = match else_block {
            Some(id) => self.ast.stmt(id).span.end,
            None => self.ast.stmt(then_block).span.end,
        };

        Some(self.ast.push_stmt(Stmt {
            kind: StmtKind::If {
                condition,
                then_block,
                else_block,
            },
            span: self.span(kw.span.start, end),
        }))
    }

    fn parse_while(&mut self) -> Option<StmtId> {
        let kw = self.advance();
        let condition = self.parse_paren_condition()?;
        let body = self.parse_control_body()?;
        let body_end = self.ast.stmt(body).span.end;

        Some(self.ast.push_stmt(Stmt {
            kind: StmtKind::While { condition, body },
            span: self.span(kw.span.start, body_end),
        }))
    }

    // Disambiguates C-style for and for-of via non-consuming lookahead for `of`.
    fn parse_for(&mut self) -> Option<StmtId> {
        let kw = self.advance();
        if !matches!(self.peek().kind, TokenKind::LeftParen) {
            self.error_at_peek("expected `(` after `for`");
            return None;
        }
        self.advance();

        if self.scan_for_of_shape() {
            self.parse_for_of_tail(kw)
        } else {
            self.parse_c_for_tail(kw)
        }
    }

    // Counts bracket/angle depth to skip past optional type annotations.
    fn scan_for_of_shape(&self) -> bool {
        if !matches!(self.peek().kind, TokenKind::Let | TokenKind::Const) {
            return false;
        }
        // Object patterns are tentatively accepted here; `parse_for_of_tail` rejects them.
        let (start, mut depth) = match self.peek_at(1).kind {
            TokenKind::Identifier => match self.peek_at(2).kind {
                TokenKind::Identifier if self.peek_at_is_word(2, "of") => return true,
                TokenKind::Colon => (3, 0i32),
                _ => return false,
            },
            TokenKind::LeftBracket | TokenKind::LeftBrace => (2, 1i32),
            _ => return false,
        };
        let mut i = start;
        loop {
            match self.peek_at(i).kind {
                TokenKind::Eof => return false,
                TokenKind::LessThan
                | TokenKind::LeftParen
                | TokenKind::LeftBracket
                | TokenKind::LeftBrace => {
                    depth += 1;
                }
                TokenKind::GreaterThan
                | TokenKind::RightParen
                | TokenKind::RightBracket
                | TokenKind::RightBrace => {
                    if depth == 0 {
                        return false;
                    }
                    depth -= 1;
                }
                // A user type literally named `of` in the annotation would
                // misclassify here and error loudly downstream — acceptable.
                TokenKind::Identifier if depth == 0 && self.peek_at_is_word(i, "of") => {
                    return true;
                }
                TokenKind::Equals | TokenKind::Semicolon if depth == 0 => return false,
                _ => {}
            }
            i += 1;
        }
    }

    // The trailing `;` of init-statement parsers doubles as the first `;` of the for header.
    fn parse_c_for_tail(&mut self, kw: Token) -> Option<StmtId> {
        let init = if matches!(self.peek().kind, TokenKind::Semicolon) {
            self.advance();
            None
        } else {
            let init_id = match self.peek().kind {
                TokenKind::Let => self.parse_let_or_const(false)?,
                TokenKind::Const => self.parse_let_or_const(true)?,
                TokenKind::Identifier if is_assign_lookahead(&self.peek_at(1).kind) => {
                    self.parse_assign()?
                }
                _ => self.parse_expression_statement()?,
            };
            Some(init_id)
        };

        let condition = if matches!(self.peek().kind, TokenKind::Semicolon) {
            None
        } else {
            Some(self.parse_expression()?)
        };
        if !matches!(self.peek().kind, TokenKind::Semicolon) {
            self.error_at_peek("expected `;` after `for` condition");
            return None;
        }
        self.advance();

        let update = if matches!(self.peek().kind, TokenKind::RightParen) {
            None
        } else {
            Some(self.parse_for_update_stmt()?)
        };
        if !matches!(self.peek().kind, TokenKind::RightParen) {
            self.error_at_peek("expected `)` after `for` update");
            return None;
        }
        self.advance();

        let body = self.parse_control_body()?;
        let body_end = self.ast.stmt(body).span.end;
        Some(self.ast.push_stmt(Stmt {
            kind: StmtKind::For {
                init,
                condition,
                update,
                body,
            },
            span: self.span(kw.span.start, body_end),
        }))
    }

    fn parse_for_update_stmt(&mut self) -> Option<StmtId> {
        if matches!(self.peek().kind, TokenKind::Identifier)
            && is_assign_lookahead(&self.peek_at(1).kind)
        {
            let name_tok = self.advance();
            let target = self.ident_from_token(&name_tok);
            let op_tok = self.advance();
            let value = self.parse_expression()?;
            let span = self.span(target.span.start, self.ast.expr(value).span.end);
            let kind = if let Some(op) = compound_op_for_token(&op_tok.kind) {
                StmtKind::CompoundAssign {
                    target,
                    op,
                    op_span: op_tok.span,
                    value,
                }
            } else {
                StmtKind::Assign { target, value }
            };
            return Some(self.ast.push_stmt(Stmt { kind, span }));
        }
        let expr_id = self.parse_expression()?;
        let expr_span = self.ast.expr(expr_id).span;
        if is_assign_lookahead(&self.peek().kind) {
            let op_tok = self.advance();
            let value_id = self.parse_expression()?;
            let span = self.span(expr_span.start, self.ast.expr(value_id).span.end);
            let compound = compound_op_for_token(&op_tok.kind);
            let lhs_kind = self.ast.expr(expr_id).kind.clone();
            return match lhs_kind {
                ExprKind::FieldAccess { receiver, name } => {
                    let kind = if let Some(op) = compound {
                        StmtKind::CompoundAssignField {
                            receiver,
                            field_name: name,
                            op,
                            op_span: op_tok.span,
                            value: value_id,
                        }
                    } else {
                        StmtKind::AssignField {
                            receiver,
                            field_name: name,
                            value: value_id,
                        }
                    };
                    Some(self.ast.push_stmt(Stmt { kind, span }))
                }
                ExprKind::IndexAccess { receiver, index } => {
                    let kind = if let Some(op) = compound {
                        StmtKind::CompoundAssignIndex {
                            receiver,
                            index,
                            op,
                            op_span: op_tok.span,
                            value: value_id,
                        }
                    } else {
                        StmtKind::AssignIndex {
                            receiver,
                            index,
                            value: value_id,
                        }
                    };
                    Some(self.ast.push_stmt(Stmt { kind, span }))
                }
                _ => {
                    self.error_at(expr_span, "invalid assignment target");
                    None
                }
            };
        }
        Some(self.ast.push_stmt(Stmt {
            kind: StmtKind::Expr(expr_id),
            span: expr_span,
        }))
    }

    fn parse_for_of_tail(&mut self, kw: Token) -> Option<StmtId> {
        let binding_tok = self.advance();
        let binding_kind = match binding_tok.kind {
            TokenKind::Let => crate::BindingKind::Let,
            TokenKind::Const => crate::BindingKind::Const,
            _ => unreachable!("scan_for_of_shape gated this"),
        };
        let (name, binding) = match self.peek().kind {
            TokenKind::LeftBracket => {
                let b = self.parse_binding()?;
                (None, Some(b))
            }
            TokenKind::LeftBrace => {
                let b = self.parse_binding()?;
                self.error_at(
                    b.span(),
                    "object destructuring is not supported in `for-of`; use array destructuring or unpack inside the loop body",
                );
                return None;
            }
            _ => {
                let name_tok =
                    self.expect_identifier("expected identifier after `let`/`const` in `for-of`")?;
                (Some(self.ident_from_token(&name_tok)), None)
            }
        };
        let ty = if matches!(self.peek().kind, TokenKind::Colon) {
            self.advance();
            Some(self.parse_type_annotation()?)
        } else {
            None
        };
        if !self.peek_identifier_text_is("of") {
            self.error_at_peek("expected `of` in `for-of`");
            return None;
        }
        self.advance();

        let iter = self.parse_expression()?;
        if !matches!(self.peek().kind, TokenKind::RightParen) {
            self.error_at_peek("expected `)` after `for-of` iterable");
            return None;
        }
        self.advance();

        let body = self.parse_control_body()?;
        let body_end = self.ast.stmt(body).span.end;
        let kind = match (binding, name) {
            (Some(binding), _) => StmtKind::ForOfPattern {
                binding_kind,
                binding,
                ty,
                iter,
                body,
            },
            (None, Some(name)) => StmtKind::ForOf {
                binding_kind,
                name,
                ty,
                iter,
                body,
            },
            (None, None) => unreachable!("either binding or name must be Some"),
        };
        Some(self.ast.push_stmt(Stmt {
            kind,
            span: self.span(kw.span.start, body_end),
        }))
    }

    fn parse_do_while(&mut self) -> Option<StmtId> {
        let kw = self.advance();
        let body = self.parse_control_body()?;
        self.eat_asi_semicolon_before(TokenKind::While);
        if !matches!(self.peek().kind, TokenKind::While) {
            self.error_at_peek("expected `while` after `do` body");
            return None;
        }
        self.advance();
        let condition = self.parse_paren_condition()?;
        // ECMAScript inserts the terminator after a do-while's `)` unconditionally, so the
        // `;` is optional here — ASI can't supply it, since that `)` closes a `while`
        // header everywhere else.
        let end = if matches!(self.peek().kind, TokenKind::Semicolon) {
            self.advance().span.end
        } else {
            self.ast.expr(condition).span.end
        };
        Some(self.ast.push_stmt(Stmt {
            kind: StmtKind::DoWhile { body, condition },
            span: self.span(kw.span.start, end),
        }))
    }

    // Multiple consecutive `case`/`default` labels share the next body.
    fn parse_switch(&mut self) -> Option<StmtId> {
        let kw = self.advance();
        if !matches!(self.peek().kind, TokenKind::LeftParen) {
            self.error_at_peek("expected `(` after `switch`");
            return None;
        }
        self.advance();
        let discriminant = self.parse_expression()?;
        if !matches!(self.peek().kind, TokenKind::RightParen) {
            self.error_at_peek("expected `)` after `switch` discriminant");
            return None;
        }
        self.advance();

        if !matches!(self.peek().kind, TokenKind::LeftBrace) {
            self.error_at_peek("expected `{` to start `switch` body");
            return None;
        }
        let open = self.advance();
        self.block_depth += 1;

        let mut cases: Vec<SwitchCase> = Vec::new();
        let mut default: Option<SwitchDefault> = None;
        let mut pending_values: Vec<ExprId> = Vec::new();
        let mut pending_label_start: Option<u32> = None;
        let mut pending_default: Option<Span> = None;

        loop {
            match self.peek().kind {
                TokenKind::Eof => break,
                // ASI inserts `;` after `:` on a newline — treat it as a no-op here.
                TokenKind::Semicolon => {
                    self.advance();
                }
                TokenKind::RightBrace => {
                    // Case bodies are required (typechecker enforces break/return termination).
                    if pending_label_start.is_some() {
                        self.error_at_peek(
                            "expected case body before `}` — case labels must be followed by statements",
                        );
                        return None;
                    }
                    break;
                }
                TokenKind::Case => {
                    let case_kw = self.advance();
                    if pending_label_start.is_none() {
                        pending_label_start = Some(case_kw.span.start);
                    }
                    let value = self.parse_expression()?;
                    if !matches!(self.peek().kind, TokenKind::Colon) {
                        self.error_at_peek("expected `:` after `case` label");
                        return None;
                    }
                    self.advance();
                    pending_values.push(value);
                }
                TokenKind::Default => {
                    let kw_tok = self.advance();
                    if default.is_some() {
                        self.error_at(kw_tok.span, "duplicate `default` clause in `switch`");
                        return None;
                    }
                    if !matches!(self.peek().kind, TokenKind::Colon) {
                        self.error_at_peek("expected `:` after `default`");
                        return None;
                    }
                    self.advance();
                    if pending_label_start.is_none() {
                        pending_label_start = Some(kw_tok.span.start);
                    }
                    pending_default = Some(kw_tok.span);
                }
                _ => {
                    if pending_label_start.is_none() {
                        self.error_at_peek(
                            "expected `case` or `default` at the start of a `switch` body",
                        );
                        return None;
                    }
                    let body = self.parse_switch_arm_body(pending_label_start.unwrap())?;
                    let arm_span = self.ast.stmt(body).span;
                    if !pending_values.is_empty() {
                        let case_span =
                            self.span(self.ast.expr(pending_values[0]).span.start, arm_span.end);
                        cases.push(SwitchCase {
                            values: std::mem::take(&mut pending_values),
                            body,
                            span: case_span,
                        });
                    }
                    if let Some(def_kw_span) = pending_default.take() {
                        default = Some(SwitchDefault {
                            body,
                            span: self.span(def_kw_span.start, arm_span.end),
                        });
                    }
                    pending_label_start = None;
                }
            }
        }

        self.block_depth -= 1;
        if !matches!(self.peek().kind, TokenKind::RightBrace) {
            self.error_at_peek("expected `}` to close `switch` body");
            return None;
        }
        let close = self.advance();

        let _ = open;
        Some(self.ast.push_stmt(Stmt {
            kind: StmtKind::Switch {
                discriminant,
                cases,
                default,
            },
            span: self.span(kw.span.start, close.span.end),
        }))
    }

    // `body_start` is the label keyword's offset so diagnostics point at the label.
    fn parse_switch_arm_body(&mut self, body_start: u32) -> Option<StmtId> {
        let mut stmts: Vec<StmtId> = Vec::new();
        let mut end_offset = body_start;
        loop {
            if matches!(
                self.peek().kind,
                TokenKind::Case | TokenKind::Default | TokenKind::RightBrace | TokenKind::Eof
            ) {
                break;
            }
            if matches!(self.peek().kind, TokenKind::Semicolon) {
                end_offset = self.peek().span.end;
                self.advance();
                continue;
            }
            let pos_before = self.pos;
            if let Some(id) = self.parse_statement() {
                end_offset = self.ast.stmt(id).span.end;
                stmts.push(id);
            } else {
                self.recover();
                if self.pos == pos_before && !self.is_at_eof() {
                    self.advance();
                }
                // Recovery consumed tokens but produced no statement; advance the body
                // end past them so the arm span can't end before its own case value.
                end_offset = end_offset.max(self.prev_token_end());
            }
        }
        let span = self.span(body_start, end_offset);
        Some(self.ast.push_stmt(Stmt {
            kind: StmtKind::Block(stmts),
            span,
        }))
    }

    fn parse_break(&mut self) -> Option<StmtId> {
        let kw = self.advance();
        if !matches!(self.peek().kind, TokenKind::Semicolon) {
            self.error_at_peek("expected `;` after `break`");
            return None;
        }
        let semi = self.advance();
        Some(self.ast.push_stmt(Stmt {
            kind: StmtKind::Break,
            span: self.span(kw.span.start, semi.span.end),
        }))
    }

    fn parse_continue(&mut self) -> Option<StmtId> {
        let kw = self.advance();
        if !matches!(self.peek().kind, TokenKind::Semicolon) {
            self.error_at_peek("expected `;` after `continue`");
            return None;
        }
        let semi = self.advance();
        Some(self.ast.push_stmt(Stmt {
            kind: StmtKind::Continue,
            span: self.span(kw.span.start, semi.span.end),
        }))
    }

    fn parse_return(&mut self) -> Option<StmtId> {
        let kw = self.advance();

        let value = if matches!(self.peek().kind, TokenKind::Semicolon) {
            None
        } else {
            Some(self.parse_expression()?)
        };

        if !matches!(self.peek().kind, TokenKind::Semicolon) {
            self.error_at_peek("expected `;` after return");
            return None;
        }
        let semi = self.advance();

        Some(self.ast.push_stmt(Stmt {
            kind: StmtKind::Return(value),
            span: self.span(kw.span.start, semi.span.end),
        }))
    }

    fn parse_throw(&mut self) -> Option<StmtId> {
        let kw = self.advance();

        if matches!(self.peek().kind, TokenKind::Semicolon) {
            self.error_at_peek_with_help(
                "expected expression after `throw`",
                vec!["throw new Error(\"message\");".to_string()],
            );
            return None;
        }

        let value = self.parse_expression()?;

        if !matches!(self.peek().kind, TokenKind::Semicolon) {
            self.error_at_peek("expected `;` after `throw`");
            return None;
        }
        let semi = self.advance();

        Some(self.ast.push_stmt(Stmt {
            kind: StmtKind::Throw { value },
            span: self.span(kw.span.start, semi.span.end),
        }))
    }

    fn parse_try(&mut self) -> Option<StmtId> {
        let kw = self.advance();

        let body = self.parse_block()?;

        let mut catches: Vec<CatchClause> = Vec::new();
        let mut finally: Option<StmtId> = None;
        let mut tail_end = self.ast.stmt(body).span.end;

        loop {
            match self.peek().kind {
                TokenKind::Catch => {
                    if finally.is_some() {
                        self.error_at_peek("`catch` clause must appear before `finally`");
                        return None;
                    }
                    let clause = self.parse_catch_clause()?;
                    tail_end = clause.span.end;
                    catches.push(clause);
                }
                TokenKind::Finally => {
                    if finally.is_some() {
                        self.error_at_peek("duplicate `finally` clause");
                        return None;
                    }
                    self.advance();
                    let block = self.parse_block()?;
                    tail_end = self.ast.stmt(block).span.end;
                    finally = Some(block);
                }
                _ => break,
            }
        }

        if catches.is_empty() && finally.is_none() {
            self.error_at(
                self.span(kw.span.start, tail_end),
                "try requires at least one of `catch` or `finally`",
            );
            return None;
        }

        Some(self.ast.push_stmt(Stmt {
            kind: StmtKind::Try {
                body,
                catches,
                finally,
            },
            span: self.span(kw.span.start, tail_end),
        }))
    }

    fn parse_catch_clause(&mut self) -> Option<CatchClause> {
        let kw = self.advance();

        let (binding, ty) = if matches!(self.peek().kind, TokenKind::LeftBrace) {
            // This binding is inaccessible to source code, including nested catches.
            (
                Ident {
                    name: "#catch".into(),
                    span: kw.span,
                },
                None,
            )
        } else {
            self.parse_catch_binding()?
        };

        let body = self.parse_block()?;
        let body_end = self.ast.stmt(body).span.end;

        Some(CatchClause {
            binding,
            ty,
            body,
            span: self.span(kw.span.start, body_end),
        })
    }

    fn parse_catch_binding(&mut self) -> Option<(Ident, Option<TypeAnnotation>)> {
        if !matches!(self.peek().kind, TokenKind::LeftParen) {
            self.error_at_peek("expected `(` after `catch`");
            return None;
        }
        self.advance();
        let name_tok = self.expect_identifier("expected catch binding name")?;
        let binding = self.ident_from_token(&name_tok);
        let ty = if matches!(self.peek().kind, TokenKind::Colon) {
            self.advance();
            Some(self.parse_type_annotation()?)
        } else {
            None
        };
        if !matches!(self.peek().kind, TokenKind::RightParen) {
            self.error_at_peek("expected `)` after catch binding");
            return None;
        }
        self.advance();
        Some((binding, ty))
    }

    fn parse_paren_condition(&mut self) -> Option<ExprId> {
        if !matches!(self.peek().kind, TokenKind::LeftParen) {
            self.error_at_peek("expected `(`");
            return None;
        }
        self.advance();
        let expr = self.parse_expression()?;
        if !matches!(self.peek().kind, TokenKind::RightParen) {
            self.error_at_peek("expected `)`");
            return None;
        }
        self.advance();
        Some(expr)
    }

    fn parse_predicate_or_return_type(
        &mut self,
        type_pos: TypePos,
    ) -> Option<(
        Option<TypeAnnotation>,
        Option<crate::TypePredicateAnnotation>,
    )> {
        if matches!(self.peek().kind, TokenKind::Identifier) && self.peek_at_is_word(1, "is") {
            let param_tok = self.advance();
            let param = self.ident_from_token(&param_tok);
            self.advance();
            let asserted = self.parse_type_annotation_in(type_pos)?;
            let span = self.span(param.span.start, asserted.span.end);
            return Some((
                None,
                Some(crate::TypePredicateAnnotation {
                    param,
                    asserted,
                    span,
                }),
            ));
        }
        Some((Some(self.parse_type_annotation_in(type_pos)?), None))
    }

    fn parse_type_annotation(&mut self) -> Option<TypeAnnotation> {
        self.parse_type_annotation_in(TypePos::Anywhere)
    }

    fn parse_type_annotation_in(&mut self, type_pos: TypePos) -> Option<TypeAnnotation> {
        // A leading `|` (prettier's multiline format) is accepted and discarded.
        let leading_pipe = matches!(self.peek().kind, TokenKind::Pipe);
        if leading_pipe {
            self.advance();
        }
        let first = self.parse_type_array(type_pos)?;
        if !matches!(self.peek().kind, TokenKind::Pipe) {
            return Some(first);
        }
        let start = first.span.start;
        let mut members = vec![first];
        while matches!(self.peek().kind, TokenKind::Pipe) {
            self.advance();
            members.push(self.parse_type_array(type_pos)?);
        }
        let end = members.last().expect("≥1 member").span.end;
        Some(TypeAnnotation {
            kind: TypeAnnotationKind::Union(members),
            span: self.span(start, end),
        })
    }

    fn parse_type_array(&mut self, type_pos: TypePos) -> Option<TypeAnnotation> {
        let mut ty = match self.peek().kind {
            TokenKind::Identifier | TokenKind::Void => {
                // `void` never starts a qualified path.
                let leading_is_identifier = matches!(self.peek().kind, TokenKind::Identifier);
                let tok = self.advance();
                let first_span = tok.span;
                let mut path: Vec<Span> = vec![first_span];
                if leading_is_identifier {
                    while matches!(self.peek().kind, TokenKind::Dot) {
                        self.advance();
                        if !matches!(self.peek().kind, TokenKind::Identifier) {
                            self.error_at_peek("expected identifier after `.` in type name");
                            return None;
                        }
                        let seg = self.advance();
                        path.push(seg.span);
                    }
                }
                let (args, end) = if matches!(self.peek().kind, TokenKind::LessThan) {
                    self.parse_type_argument_list()?
                } else {
                    (Vec::new(), path.last().unwrap().end)
                };
                let outer_span = self.span(first_span.start, end);
                let kind = if path.len() == 1 {
                    if &self.source[first_span.start as usize..first_span.end as usize] == "any" {
                        self.error_at_with_help(
                            first_span,
                            "`any` is not supported",
                            vec![
                                "annotate a concrete type instead — an object shape \
                                 like `{ id: string }`, a named `interface`, or \
                                 `unknown` (a safe dynamic type you narrow before use)"
                                    .to_string(),
                            ],
                        );
                    }
                    TypeAnnotationKind::Name {
                        name_span: first_span,
                        args,
                    }
                } else {
                    TypeAnnotationKind::Qualified { path, args }
                };
                TypeAnnotation {
                    kind,
                    span: outer_span,
                }
            }
            // `null<T>` is meaningless — no type args after `null`.
            TokenKind::NullLiteral => {
                let tok = self.advance();
                TypeAnnotation {
                    kind: TypeAnnotationKind::Name {
                        name_span: tok.span,
                        args: Vec::new(),
                    },
                    span: tok.span,
                }
            }
            TokenKind::StringLiteral(_) => {
                let tok = self.advance();
                let span = tok.span;
                let TokenKind::StringLiteral(s) = tok.kind else {
                    unreachable!("just matched StringLiteral");
                };
                TypeAnnotation {
                    kind: TypeAnnotationKind::StringLiteral(s),
                    span,
                }
            }
            TokenKind::NumberLiteral(_) => {
                let tok = self.advance();
                let span = tok.span;
                let TokenKind::NumberLiteral(v) = tok.kind else {
                    unreachable!("just matched NumberLiteral");
                };
                // Canonicalize `-0.0` → `0.0` so literal type `0` matches both signs.
                let canonical = if v == 0.0 { 0.0 } else { v };
                TypeAnnotation {
                    kind: TypeAnnotationKind::NumberLiteral(crate::types::LiteralF64(canonical)),
                    span,
                }
            }
            TokenKind::LeftBrace => self.parse_object_type_annotation()?,
            TokenKind::LeftParen if self.paren_at_opens_function_type(self.pos, type_pos) => {
                self.parse_function_type_annotation()?
            }
            TokenKind::LeftParen => self.parse_grouped_or_function_type()?,
            // Leading `[` is unambiguously a tuple (postfix `[]` can only follow a base type).
            TokenKind::LeftBracket => self.parse_tuple_type_annotation()?,
            _ => {
                self.error_at_peek("expected type");
                return None;
            }
        };
        while matches!(self.peek().kind, TokenKind::LeftBracket) {
            let open = self.advance();
            if !matches!(self.peek().kind, TokenKind::RightBracket) {
                self.error_at(open.span, "expected `]` to close array type");
                return None;
            }
            let close = self.advance();
            let span = self.span(ty.span.start, close.span.end);
            ty = TypeAnnotation {
                kind: TypeAnnotationKind::Array(Box::new(ty)),
                span,
            };
        }
        Some(ty)
    }

    /// In type position `(` is ambiguous: a function type's parameter list, or a
    /// grouping paren the postfix `[]` / `|` then composes with. Decided by what the
    /// parens *contain*, not by what follows them — a trailing `=>` can belong to an
    /// enclosing arrow function's body (`(): (string | null) => null`), so looking
    /// past the `)` would read that return type as a parameter list. The exception is
    /// `(T)` / `(T, U)`, spelled identically either way, which only a trailing `=>`
    /// can break.
    ///
    /// Empty parens count as a parameter list: `()` groups nothing, so the
    /// function-type parse's "expected `=>`" is the message that names the fix.
    fn paren_at_opens_function_type(&self, index: usize, type_pos: TypePos) -> bool {
        let after = |offset: usize| self.tokens.get(index + offset).map(|t| &t.kind);
        match (after(1), after(2)) {
            // Only a parameter list is empty or starts with a rest element.
            (Some(TokenKind::RightParen | TokenKind::DotDotDot), _) => true,
            // A named, annotated parameter — no type starts this way.
            (Some(TokenKind::Identifier), Some(TokenKind::Colon | TokenKind::Question)) => true,
            // `(T)` and `(T, U)` are spelled exactly like a grouped type. v1
            // rejects bare parameter names, and a trailing `=>` is the only sign
            // the writer meant one — routing those to the function-type parse
            // keeps its "params require named annotations" message reachable in
            // declaration position (`type F = (T) => R`). In an arrow's *return*
            // annotation the trailing `=>` is the body's, so `(): (T) => x` loses
            // that message and falls back to the expression parse; dropping the
            // redundant parens is the fix there.
            (Some(TokenKind::Identifier), Some(TokenKind::Comma | TokenKind::RightParen)) => {
                type_pos == TypePos::Anywhere
                    && self.index_after_matching_paren(index).is_some_and(|i| {
                        matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Arrow))
                    })
            }
            // Anything else can only begin a type, so the parens group.
            _ => false,
        }
    }

    /// Parse a `(` the contents rule called a group. If that fails and a `=>` follows
    /// the matching `)`, the author was writing a function type after all — a typo in
    /// its parameter list is what made it look like a group — so re-parse that way to
    /// recover the diagnostic naming the parameter rule instead of a stray-`)` error.
    fn parse_grouped_or_function_type(&mut self) -> Option<TypeAnnotation> {
        let open = self.pos;
        let diagnostics_before = self.diagnostics.len();
        if let Some(ty) = self.parse_grouped_type_annotation() {
            return Some(ty);
        }
        let arrow_follows = self
            .index_after_matching_paren(open)
            .is_some_and(|i| matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Arrow)));
        if !arrow_follows {
            return None;
        }
        self.pos = open;
        self.diagnostics.truncate(diagnostics_before);
        self.parse_function_type_annotation()
    }

    /// A parenthesized type is its inner type — the parens only group, so no node
    /// records them. The span covers them so a diagnostic points at what was written.
    fn parse_grouped_type_annotation(&mut self) -> Option<TypeAnnotation> {
        let open = self.advance();
        let inner = self.parse_type_annotation()?;
        if !matches!(self.peek().kind, TokenKind::RightParen) {
            self.error_at_peek("expected `)` to close a parenthesized type");
            return None;
        }
        let close = self.advance();
        Some(TypeAnnotation {
            span: self.span(open.span.start, close.span.end),
            ..inner
        })
    }

    // Param names are required — bare `(T, U) => R` is rejected.
    fn parse_function_type_annotation(&mut self) -> Option<TypeAnnotation> {
        let open = self.advance();
        let params = self.parse_function_type_params()?;
        if !matches!(self.peek().kind, TokenKind::Arrow) {
            self.error_at_peek("expected `=>` in function-type annotation");
            return None;
        }
        self.advance();
        let return_type = self.parse_type_annotation()?;
        let end = return_type.span.end;
        Some(TypeAnnotation {
            kind: TypeAnnotationKind::Function {
                params,
                return_type: Box::new(return_type),
            },
            span: self.span(open.span.start, end),
        })
    }

    /// The `(…)` of a function type, its `(` already consumed and its `)` consumed here.
    /// Shared with a type literal's method members, which spell the same parameter list
    /// before a `:` rather than a `=>`.
    fn parse_function_type_params(&mut self) -> Option<Vec<TypeAnnotationField>> {
        let mut params: Vec<TypeAnnotationField> = Vec::new();
        if !matches!(self.peek().kind, TokenKind::RightParen) {
            loop {
                let rest = if matches!(self.peek().kind, TokenKind::DotDotDot) {
                    self.advance();
                    true
                } else {
                    false
                };
                let name_tok = self.expect_identifier(if rest {
                    "expected identifier after `...` in function-type annotation"
                } else {
                    "expected parameter name in function-type annotation"
                })?;
                let name = self.ident_from_token(&name_tok);
                if matches!(self.peek().kind, TokenKind::Question) {
                    self.error_at_peek("optional function parameters are not yet supported");
                    return None;
                }
                if !matches!(self.peek().kind, TokenKind::Colon) {
                    if rest {
                        self.error_at_peek_with_help(
                            "rest parameter requires a type annotation",
                            vec!["(...args: T[]) => R".to_string()],
                        );
                    } else {
                        self.error_at_peek(
                            "expected `:` after parameter name (function-type params \
                             require named annotations in v1)",
                        );
                    }
                    return None;
                }
                self.advance();
                let ty = self.parse_type_annotation()?;
                if rest && !matches!(ty.kind, crate::ast::TypeAnnotationKind::Array(_)) {
                    self.error_at(ty.span, "rest parameter type must be an array");
                    return None;
                }
                if matches!(self.peek().kind, TokenKind::Equals) {
                    if rest {
                        self.error_at_peek_with_help(
                            "rest parameter cannot have a default value",
                            vec![
                                "omit the `= …`; an unspecified rest defaults to `[]`".to_string(),
                            ],
                        );
                    } else {
                        self.error_at_peek(
                            "default values in function-type annotations are not yet supported",
                        );
                    }
                    return None;
                }
                params.push(TypeAnnotationField {
                    name,
                    ty,
                    optional: false,
                    readonly: false,
                    rest,
                });
                match self.peek().kind {
                    TokenKind::Comma => {
                        if rest {
                            let comma = self.advance();
                            self.error_at(comma.span, "rest parameter must be the last parameter");
                            return None;
                        }
                        self.advance();
                        if matches!(self.peek().kind, TokenKind::RightParen) {
                            break;
                        }
                    }
                    TokenKind::RightParen => break,
                    _ => {
                        self.error_at_peek("expected `,` or `)`");
                        return None;
                    }
                }
            }
        }
        if !matches!(self.peek().kind, TokenKind::RightParen) {
            self.error_at_peek("expected `)`");
            return None;
        }
        self.advance();
        Some(params)
    }

    fn parse_tuple_type_annotation(&mut self) -> Option<TypeAnnotation> {
        let open = self.advance();
        if matches!(self.peek().kind, TokenKind::RightBracket) {
            let close = self.peek().span;
            self.error_at(
                self.span(open.span.start, close.end),
                "tuple types must have at least one element",
            );
            return None;
        }
        let mut elements: Vec<TypeAnnotation> = Vec::new();
        loop {
            let ty = self.parse_type_annotation()?;
            elements.push(ty);
            match self.peek().kind {
                TokenKind::Comma => {
                    self.advance();
                    if matches!(self.peek().kind, TokenKind::RightBracket) {
                        break;
                    }
                }
                TokenKind::RightBracket => break,
                _ => {
                    self.error_at_peek("expected `,` or `]`");
                    return None;
                }
            }
        }
        if !matches!(self.peek().kind, TokenKind::RightBracket) {
            self.error_at_peek("expected `]`");
            return None;
        }
        let close = self.advance();
        Some(TypeAnnotation {
            kind: TypeAnnotationKind::Tuple(elements),
            span: self.span(open.span.start, close.span.end),
        })
    }

    fn parse_object_type_annotation(&mut self) -> Option<TypeAnnotation> {
        let open = self.advance();
        let mut fields: Vec<TypeAnnotationField> = Vec::new();
        if !matches!(self.peek().kind, TokenKind::RightBrace) {
            loop {
                let readonly = self.eat_readonly_property_modifier();
                let name = self.expect_property_ident("expected field name in object type")?;
                // `name?: T` — omittable at construction; reads widen to `T | null`.
                let optional = matches!(self.peek().kind, TokenKind::Question);
                if optional {
                    self.advance();
                }
                let ty = if matches!(self.peek().kind, TokenKind::LeftParen) {
                    self.parse_object_type_method_signature(name.span.start)?
                } else {
                    if !matches!(self.peek().kind, TokenKind::Colon) {
                        self.error_at_peek("expected `:` after field name");
                        return None;
                    }
                    self.advance();
                    self.parse_type_annotation()?
                };
                if let Some(existing) = fields.iter().find(|f| f.name.name == name.name) {
                    self.diagnostics.push(Diagnostic {
                        severity: Severity::Error,
                        span: name.span,
                        message: format!("duplicate field `{}` in object type", name.name),
                        help: vec![],
                        notes: vec![(existing.name.span, "first defined here".into())],
                    });
                }
                fields.push(TypeAnnotationField {
                    name,
                    ty,
                    optional,
                    readonly,
                    rest: false,
                });
                match self.peek().kind {
                    TokenKind::Semicolon | TokenKind::Comma => {
                        self.advance();
                        if matches!(self.peek().kind, TokenKind::RightBrace) {
                            break;
                        }
                    }
                    TokenKind::RightBrace => break,
                    _ => {
                        self.error_at_peek("expected `;`, `,`, or `}`");
                        return None;
                    }
                }
            }
        }
        if !matches!(self.peek().kind, TokenKind::RightBrace) {
            self.error_at_peek("expected `}`");
            return None;
        }
        let close = self.advance();
        Some(TypeAnnotation {
            kind: TypeAnnotationKind::Object { fields },
            span: self.span(open.span.start, close.span.end),
        })
    }

    /// `m(params): T` inside a type literal, spelled as the function-typed field
    /// `m: (params) => T` — the two are interchangeable in a structural shape, and
    /// interfaces already accept both spellings for the same member. `start` is the
    /// member name's offset, so the annotation's span covers the name too.
    fn parse_object_type_method_signature(&mut self, start: u32) -> Option<TypeAnnotation> {
        self.advance();
        let params = self.parse_function_type_params()?;
        if !matches!(self.peek().kind, TokenKind::Colon) {
            self.error_at_peek("expected `:` and return type after method parameters");
            return None;
        }
        self.advance();
        let return_type = self.parse_type_annotation()?;
        let end = return_type.span.end;
        Some(TypeAnnotation {
            kind: TypeAnnotationKind::Function {
                params,
                return_type: Box::new(return_type),
            },
            span: self.span(start, end),
        })
    }

    fn expect_identifier(&mut self, message: &str) -> Option<Token> {
        if matches!(self.peek().kind, TokenKind::Identifier) {
            return Some(self.advance());
        }
        if is_reserved_identifier_word(&self.peek().kind) {
            let tok = self.advance();
            let keyword = &self.source[tok.span.start as usize..tok.span.end as usize];
            self.error_at_with_help(
                tok.span,
                format!("`{keyword}` is a reserved keyword and can't be used as a name"),
                vec![format!(
                    "rename it, for example: `{}`",
                    reserved_keyword_rename_example(keyword)
                )],
            );
            return None;
        }
        self.error_at_peek(message.to_string());
        None
    }

    fn ident_from_token(&self, tok: &Token) -> Ident {
        Ident {
            name: self.source[tok.span.start as usize..tok.span.end as usize].to_string(),
            span: tok.span,
        }
    }

    fn property_ident_from_token(&self, tok: &Token) -> Ident {
        if let TokenKind::StringLiteral(s) = &tok.kind {
            Ident {
                name: s.clone(),
                span: tok.span,
            }
        } else {
            self.ident_from_token(tok)
        }
    }

    fn expect_property_name(&mut self, message: &str) -> Option<Token> {
        if !is_property_name(&self.peek().kind) {
            self.error_at_peek(message.to_string());
            return None;
        }
        Some(self.advance())
    }

    fn expect_property_ident(&mut self, message: &str) -> Option<Ident> {
        if !is_property_name(&self.peek().kind)
            && !matches!(self.peek().kind, TokenKind::StringLiteral(_))
        {
            self.error_at_peek(message.to_string());
            return None;
        }
        let tok = self.advance();
        Some(self.property_ident_from_token(&tok))
    }

    fn eat_readonly_property_modifier(&mut self) -> bool {
        if self.peek_identifier_text_is("readonly") && self.peek_starts_property_after_readonly() {
            self.advance();
            return true;
        }
        false
    }

    fn peek_starts_property_after_readonly(&self) -> bool {
        let name = &self.peek_at(1).kind;
        let after_name = &self.peek_at(2).kind;
        let after_optional = &self.peek_at(3).kind;
        // A member starts its type with `:`, or its parameter list with `(` — otherwise
        // `readonly` is the member's own name.
        let starts_member_type =
            |kind: &TokenKind| matches!(kind, TokenKind::Colon | TokenKind::LeftParen);
        (is_property_name(name) || matches!(name, TokenKind::StringLiteral(_)))
            && (starts_member_type(after_name)
                || matches!(after_name, TokenKind::Question) && starts_member_type(after_optional))
    }

    fn peek_identifier_text_is(&self, expected: &str) -> bool {
        self.token_is_word(self.peek(), expected)
    }

    fn peek_at_is_word(&self, offset: usize, expected: &str) -> bool {
        self.token_is_word(self.peek_at(offset), expected)
    }

    fn token_is_word(&self, tok: &Token, expected: &str) -> bool {
        matches!(tok.kind, TokenKind::Identifier)
            && &self.source[tok.span.start as usize..tok.span.end as usize] == expected
    }

    /// `type X …` commits to a type-alias declaration only when followed by an
    /// identifier, mirroring TypeScript; `type = 5`, `type;`, `type(x)` stay
    /// expression statements over a binding named `type`.
    fn at_type_alias_head(&self) -> bool {
        self.peek_identifier_text_is("type")
            && matches!(self.peek_at(1).kind, TokenKind::Identifier)
    }

    fn parse_expression(&mut self) -> Option<ExprId> {
        if self.is_arrow_start() {
            return self.parse_arrow();
        }
        let cond = self.parse_binary(0)?;
        // Ternary sits below every binary op; right-associative.
        if !matches!(self.peek().kind, TokenKind::Question) {
            return Some(cond);
        }
        self.advance();
        let then_ = self.parse_expression()?;
        if !matches!(self.peek().kind, TokenKind::Colon) {
            self.error_at_peek_with_help(
                "expected `:` to complete ternary expression",
                vec!["const x = cond ? then : else;".to_string()],
            );
            return None;
        }
        self.advance();
        let else_ = self.parse_expression()?;
        let cond_span = self.ast.expr(cond).span;
        let else_span = self.ast.expr(else_).span;
        Some(self.ast.push_expr(Expr {
            kind: ExprKind::Ternary { cond, then_, else_ },
            span: self.span(cond_span.start, else_span.end),
        }))
    }

    fn is_arrow_start(&self) -> bool {
        match self.peek().kind {
            TokenKind::Identifier => matches!(self.peek_at(1).kind, TokenKind::Arrow),
            TokenKind::LeftParen => self.find_arrow_after_matching_paren(),
            _ => false,
        }
    }

    /// Index just past the `)` matching the `(` at `index`; `None` when the parens
    /// never balance. Pure lookahead — emits no diagnostics.
    fn index_after_matching_paren(&self, index: usize) -> Option<usize> {
        debug_assert!(matches!(
            self.tokens.get(index).map(|t| &t.kind),
            Some(TokenKind::LeftParen)
        ));
        self.scan_past_balanced(
            index,
            |k| matches!(k, TokenKind::LeftParen),
            |k| matches!(k, TokenKind::RightParen),
        )
    }

    fn find_arrow_after_matching_paren(&self) -> bool {
        let Some(i) = self.index_after_matching_paren(self.pos) else {
            return false;
        };
        let after_type = if matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Colon)) {
            match self.scan_past_type_annotation(i + 1, TypePos::ArrowReturn) {
                Some(p) => p,
                None => return false,
            }
        } else {
            i
        };
        matches!(
            self.tokens.get(after_type).map(|t| &t.kind),
            Some(TokenKind::Arrow),
        )
    }

    // Pure lookahead used only by the arrow-disambiguator. No diagnostics.
    // Must accept every form `parse_type_annotation` accepts: a form missing
    // here makes the disambiguator reject a valid arrow, so the expression
    // parser reports a misleading `expected expression` at the return type.
    // `arrow_return_type_scanner_matches_type_grammar` pins the two together.
    fn scan_past_type_annotation(&self, start: usize, type_pos: TypePos) -> Option<usize> {
        if matches!(
            self.tokens.get(start).map(|t| &t.kind),
            Some(TokenKind::Identifier)
        ) && self
            .tokens
            .get(start + 1)
            .is_some_and(|t| self.token_is_word(t, "is"))
        {
            return self.scan_past_type_annotation(start + 2, type_pos);
        }
        // Leading `|` (prettier's multiline union format).
        let start = if matches!(
            self.tokens.get(start).map(|t| &t.kind),
            Some(TokenKind::Pipe)
        ) {
            start + 1
        } else {
            start
        };
        let mut i = self.scan_past_single_type_member(start, type_pos)?;
        while matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Pipe)) {
            i = self.scan_past_single_type_member(i + 1, type_pos)?;
        }
        Some(i)
    }

    fn scan_past_single_type_member(&self, start: usize, type_pos: TypePos) -> Option<usize> {
        let mut i = start;
        match self.tokens.get(i)?.kind {
            TokenKind::Identifier | TokenKind::Void => {
                i += 1;
                while matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Dot)) {
                    if !matches!(
                        self.tokens.get(i + 1).map(|t| &t.kind),
                        Some(TokenKind::Identifier)
                    ) {
                        return None;
                    }
                    i += 2;
                }
                if matches!(
                    self.tokens.get(i).map(|t| &t.kind),
                    Some(TokenKind::LessThan)
                ) {
                    i = self.scan_past_balanced(
                        i,
                        |k| matches!(k, TokenKind::LessThan),
                        |k| matches!(k, TokenKind::GreaterThan),
                    )?;
                }
            }
            TokenKind::NullLiteral | TokenKind::StringLiteral(_) | TokenKind::NumberLiteral(_) => {
                i += 1;
            }
            TokenKind::LeftBrace => {
                i = self.scan_past_balanced(
                    i,
                    |k| matches!(k, TokenKind::LeftBrace),
                    |k| matches!(k, TokenKind::RightBrace),
                )?;
            }
            TokenKind::LeftBracket => {
                i = self.scan_past_balanced(
                    i,
                    |k| matches!(k, TokenKind::LeftBracket),
                    |k| matches!(k, TokenKind::RightBracket),
                )?;
            }
            // Same `(`-disambiguation `parse_type_array` uses, and it has to agree:
            // a function type carries its own `=>`, so everything after it is the
            // return type and the scan ends with that. Grouping parens instead fall
            // through to the postfix `[]` loop below.
            TokenKind::LeftParen => {
                let is_function_type = self.paren_at_opens_function_type(i, type_pos);
                i = self.scan_past_balanced(
                    i,
                    |k| matches!(k, TokenKind::LeftParen),
                    |k| matches!(k, TokenKind::RightParen),
                )?;
                if is_function_type {
                    if !matches!(self.tokens.get(i).map(|t| &t.kind), Some(TokenKind::Arrow)) {
                        return None;
                    }
                    return self.scan_past_type_annotation(i + 1, TypePos::Anywhere);
                }
            }
            _ => return None,
        }
        while matches!(
            self.tokens.get(i).map(|t| &t.kind),
            Some(TokenKind::LeftBracket)
        ) {
            i += 1;
            if !matches!(
                self.tokens.get(i).map(|t| &t.kind),
                Some(TokenKind::RightBracket)
            ) {
                return None;
            }
            i += 1;
        }
        Some(i)
    }

    // `start` must index the opening token; returns the index just past the
    // matching close, or None if the tokens run out first.
    fn scan_past_balanced(
        &self,
        start: usize,
        is_open: fn(&TokenKind) -> bool,
        is_close: fn(&TokenKind) -> bool,
    ) -> Option<usize> {
        let mut depth: i32 = 1;
        let mut i = start + 1;
        while i < self.tokens.len() && depth > 0 {
            let kind = &self.tokens[i].kind;
            if is_open(kind) {
                depth += 1;
            } else if is_close(kind) {
                depth -= 1;
            } else if matches!(kind, TokenKind::Eof) {
                return None;
            }
            i += 1;
        }
        if depth == 0 { Some(i) } else { None }
    }

    fn parse_arrow(&mut self) -> Option<ExprId> {
        let start_span = self.peek().span;

        let params = match self.peek().kind {
            TokenKind::Identifier => {
                let name_tok = self.advance();
                let name = self.ident_from_token(&name_tok);
                vec![ParamDecl {
                    name,
                    ty: None,
                    default: None,
                    pattern: None,
                    rest: false,
                    modifiers: None,
                }]
            }
            TokenKind::LeftParen => {
                self.advance();
                let params = if matches!(self.peek().kind, TokenKind::RightParen) {
                    Vec::new()
                } else {
                    self.parse_arrow_param_list()?
                };
                if !matches!(self.peek().kind, TokenKind::RightParen) {
                    self.error_at_peek("expected `)`");
                    return None;
                }
                self.advance();
                params
            }
            _ => unreachable!("is_arrow_start guarded the entry"),
        };

        let (return_type, type_predicate) = if matches!(self.peek().kind, TokenKind::Colon) {
            self.advance();
            self.parse_predicate_or_return_type(TypePos::ArrowReturn)?
        } else {
            (None, None)
        };

        if !matches!(self.peek().kind, TokenKind::Arrow) {
            self.error_at_peek("expected `=>` in arrow function");
            return None;
        }
        self.advance();

        let (body, end_pos) = if matches!(self.peek().kind, TokenKind::LeftBrace) {
            let block = self.parse_block()?;
            let span = self.ast.stmt(block).span;
            (ArrowBody::Block(block), span.end)
        } else {
            let expr = self.parse_expression()?;
            let span = self.ast.expr(expr).span;
            (ArrowBody::Expr(expr), span.end)
        };

        Some(self.ast.push_expr(Expr {
            kind: ExprKind::Arrow {
                params,
                return_type,
                type_predicate,
                body,
            },
            span: self.span(start_span.start, end_pos),
        }))
    }

    /// `function (a: T): R { … }` in expression position, parsed into `ExprKind::Arrow`.
    ///
    /// The two forms differ only in how they bind `this`. The body is parsed through
    /// `parse_outer_this_boundary`, which rejects `this` inside it, so the one case where
    /// they would disagree cannot be written and the lowering is exact for the rest.
    ///
    /// In the named form `function f() { … }`, TypeScript binds `f` inside the function's
    /// own body, and an arrow has nowhere to record that name. The name is therefore
    /// accepted and dropped — it is almost always only a label — and rejected just when
    /// the body actually references it, which is the one case where dropping it would
    /// change meaning.
    fn parse_function_expression(&mut self) -> Option<ExprId> {
        let kw = self.advance();

        if matches!(self.peek().kind, TokenKind::LessThan) {
            self.error_at_peek("generic function expressions are not supported");
            return None;
        }

        let self_name = if matches!(self.peek().kind, TokenKind::Identifier) {
            let name_tok = self.advance();
            Some(self.ident_from_token(&name_tok))
        } else {
            None
        };

        if !matches!(self.peek().kind, TokenKind::LeftParen) {
            self.error_at_peek("expected `(` after `function`");
            return None;
        }
        self.advance();

        let params = if matches!(self.peek().kind, TokenKind::RightParen) {
            Vec::new()
        } else {
            // Shares the arrow parameter grammar, so parameter defaults are rejected here
            // too — only named function *declarations* accept them.
            self.parse_arrow_param_list()?
        };
        if !matches!(self.peek().kind, TokenKind::RightParen) {
            self.error_at_peek("expected `)`");
            return None;
        }
        self.advance();

        let (return_type, type_predicate) = if matches!(self.peek().kind, TokenKind::Colon) {
            self.advance();
            self.parse_predicate_or_return_type(TypePos::ArrowReturn)?
        } else {
            (None, None)
        };

        if !matches!(self.peek().kind, TokenKind::LeftBrace) {
            self.error_at_peek("expected `{` to open the function body");
            return None;
        }
        let body_start = self.pos;
        let block = self.parse_outer_this_boundary(Self::parse_block)?;
        let end = self.ast.stmt(block).span.end;

        if let Some(name) = self_name.filter(|n| self.body_mentions(body_start, &n.name)) {
            self.error_at_with_help(
                name.span,
                "a named function expression cannot call itself",
                vec![format!(
                    "declare it instead: `function {}(…) {{ … }}`",
                    name.name
                )],
            );
            return None;
        }

        Some(self.ast.push_expr(Expr {
            kind: ExprKind::Arrow {
                params,
                return_type,
                type_predicate,
                body: ArrowBody::Block(block),
            },
            span: self.span(kw.span.start, end),
        }))
    }

    /// Parses a body that gets its own `this`, so an enclosing class method's `this` does
    /// not leak into it.
    ///
    /// A `function` expression and a shorthand method each rebind `this` at call time,
    /// but both lower to `ExprKind::Arrow`, which captures `this` lexically. Inside a
    /// class method the two disagree: TypeScript gives the receiver, an arrow gives the
    /// enclosing instance. Zeroing the depth turns that case into the existing "`this` is
    /// only valid inside a class method" error instead of a silently different value.
    fn parse_outer_this_boundary<T>(
        &mut self,
        parse: impl FnOnce(&mut Self) -> Option<T>,
    ) -> Option<T> {
        let saved = self.class_member_body_depth;
        self.class_member_body_depth = 0;
        let parsed = parse(self);
        self.class_member_body_depth = saved;
        parsed
    }

    /// Whether the body starting at `body_start` spells `name` anywhere.
    ///
    /// Deliberately an over-approximation: it matches identifier tokens by text, with no
    /// notion of scope, so a property (`o.bar`), an object key (`{ bar: 1 }`) or a local
    /// that shadows the name also count. The parser has no binding information here, and
    /// erring toward rejection is the safe direction — a missed self-reference would
    /// silently drop a binding the body depends on.
    ///
    /// Two positions are excluded because they can never be a reference to the function
    /// and are common in ordinary code: a member name after `.`, and a key before `:`.
    /// A local, parameter, or type that merely shares the name is still caught.
    fn body_mentions(&self, body_start: usize, name: &str) -> bool {
        let body = &self.tokens[body_start..self.pos];
        body.iter().enumerate().any(|(i, t)| {
            if !matches!(t.kind, TokenKind::Identifier)
                || self.source[t.span.start as usize..t.span.end as usize] != *name
            {
                return false;
            }
            let after_dot = i
                .checked_sub(1)
                .is_some_and(|p| matches!(body[p].kind, TokenKind::Dot));
            let before_colon = body
                .get(i + 1)
                .is_some_and(|n| matches!(n.kind, TokenKind::Colon));
            !after_dot && !before_colon
        })
    }

    fn parse_arrow_param_list(&mut self) -> Option<Vec<ParamDecl>> {
        let mut params = Vec::new();
        loop {
            let rest = if matches!(self.peek().kind, TokenKind::DotDotDot) {
                self.advance();
                true
            } else {
                false
            };
            // The lowering pass rewrites expression-body arrows to block form when any param
            // has a destructuring pattern.
            let (name, pattern) = match self.peek().kind {
                TokenKind::LeftBrace | TokenKind::LeftBracket => {
                    if rest {
                        self.error_at_peek("rest parameter cannot be destructured");
                        return None;
                    }
                    let binding = self.parse_binding()?;
                    let placeholder = Ident {
                        name: String::new(),
                        span: binding.span(),
                    };
                    (placeholder, Some(binding))
                }
                _ => {
                    let name_tok = self.expect_identifier(if rest {
                        "expected identifier after `...`"
                    } else {
                        "expected parameter name"
                    })?;
                    let name = self.ident_from_token(&name_tok);
                    (name, None)
                }
            };
            let ty = if matches!(self.peek().kind, TokenKind::Colon) {
                self.advance();
                Some(self.parse_type_annotation()?)
            } else {
                // Rest without annotation is OK at parse time; the typechecker fills it
                // from a contextual function-type hint if available.
                None
            };
            // Only named function declarations support parameter defaults. This list also
            // serves function expressions and shorthand methods, so the message names the
            // parameter rather than the construct the user wrote.
            if matches!(self.peek().kind, TokenKind::Equals) {
                if rest {
                    self.error_at_peek_with_help(
                        "rest parameter cannot have a default value",
                        vec!["omit the `= …`; an unspecified rest defaults to `[]`".to_string()],
                    );
                } else {
                    self.error_at_peek_with_help(
                        "default parameter values are only supported on function declarations",
                        vec![
                            "declare the function, or drop the default and use `??` in the body"
                                .to_string(),
                        ],
                    );
                }
                return None;
            }
            params.push(ParamDecl {
                name,
                ty,
                default: None,
                pattern,
                rest,
                modifiers: None,
            });
            match self.peek().kind {
                TokenKind::Comma => {
                    if rest {
                        let comma = self.advance();
                        self.error_at(comma.span, "rest parameter must be the last parameter");
                        return None;
                    }
                    self.advance();
                    if matches!(self.peek().kind, TokenKind::RightParen) {
                        return Some(params);
                    }
                }
                TokenKind::RightParen => return Some(params),
                _ => {
                    self.error_at_peek("expected `,` or `)`");
                    return None;
                }
            }
        }
    }

    fn parse_binary(&mut self, min_prec: u8) -> Option<ExprId> {
        let mut lhs = self.parse_cast()?;
        // Track the previous op to reject `a || b ?? c` / `a ?? b || c` mixes.
        let mut last_op: Option<BinOp> = None;
        while let Some((op, prec)) = peek_binop(&self.peek().kind) {
            if prec < min_prec {
                break;
            }
            if let Some(last) = last_op
                && mixed_logical(last, op)
            {
                let op_span = self.peek().span;
                self.error_at_with_help(
                    op_span,
                    "mixing `??` with `||` / `&&` requires parentheses",
                    vec!["wrap the side you mean: `(a || b) ?? c` or `a || (b ?? c)`".to_string()],
                );
                return None;
            }
            self.advance();
            let next_min = if is_right_associative(op) {
                prec
            } else {
                prec + 1
            };
            let rhs = self.parse_binary(next_min)?;
            let lhs_span = self.ast.expr(lhs).span;
            let rhs_span = self.ast.expr(rhs).span;
            lhs = self.ast.push_expr(Expr {
                kind: ExprKind::Binary { op, lhs, rhs },
                span: self.span(lhs_span.start, rhs_span.end),
            });
            last_op = Some(op);
        }
        Some(lhs)
    }

    // Higher than every binary op, lower than every unary prefix. Left-associative.
    fn parse_cast(&mut self) -> Option<ExprId> {
        let mut expr = self.parse_unary()?;
        while matches!(self.peek().kind, TokenKind::Instanceof)
            || self.peek_identifier_text_is("as")
        {
            let is_instanceof = matches!(self.peek().kind, TokenKind::Instanceof);
            self.advance();
            let ty = self.parse_type_annotation()?;
            let start = self.ast.expr(expr).span.start;
            let end = ty.span.end;
            let kind = if is_instanceof {
                ExprKind::InstanceOf { value: expr, ty }
            } else {
                ExprKind::As { expr, ty }
            };
            expr = self.ast.push_expr(Expr {
                kind,
                span: self.span(start, end),
            });
        }
        Some(expr)
    }

    fn parse_unary(&mut self) -> Option<ExprId> {
        // `typeof` is parsed as a unary prefix so `typeof x === "T"` works without a new layer;
        // the typechecker rejects `Typeof` outside the recognized equality fold position.
        if matches!(self.peek().kind, TokenKind::Typeof) {
            let op_tok = self.advance();
            let operand = self.parse_unary()?;
            let operand_span = self.ast.expr(operand).span;
            return Some(self.ast.push_expr(Expr {
                kind: ExprKind::Typeof { operand },
                span: self.span(op_tok.span.start, operand_span.end),
            }));
        }
        if matches!(self.peek().kind, TokenKind::New) {
            let op_tok = self.advance();
            let mut callee = self.parse_atom()?;
            while matches!(self.peek().kind, TokenKind::Dot) {
                self.advance();
                if !matches!(self.peek().kind, TokenKind::Identifier) {
                    self.error_at_peek("expected identifier after `.` in constructor name");
                    return None;
                }
                let ident_tok = self.advance();
                let name = self.ident_from_token(&ident_tok);
                let receiver_span = self.ast.expr(callee).span;
                let combined_span = self.span(receiver_span.start, ident_tok.span.end);
                callee = self.ast.push_expr(Expr {
                    kind: ExprKind::FieldAccess {
                        receiver: callee,
                        name,
                    },
                    span: combined_span,
                });
            }
            let type_args = if matches!(self.peek().kind, TokenKind::LessThan) {
                let saved_pos = self.pos;
                let saved_diag_len = self.diagnostics.len();
                if let Some(args) = self.try_parse_type_args_only() {
                    Some(args)
                } else {
                    self.pos = saved_pos;
                    self.diagnostics.truncate(saved_diag_len);
                    None
                }
            } else {
                None
            };
            if !matches!(self.peek().kind, TokenKind::LeftParen) {
                self.error_at_peek("expected `(` after constructor name in `new` expression");
                return None;
            }
            self.advance();
            let args = if matches!(self.peek().kind, TokenKind::RightParen) {
                Vec::new()
            } else {
                self.parse_call_args()?
            };
            if !matches!(self.peek().kind, TokenKind::RightParen) {
                self.error_at_peek("expected `)`");
                return None;
            }
            let close = self.advance();
            let new_expr = self.ast.push_expr(Expr {
                kind: ExprKind::New {
                    callee,
                    type_args,
                    args,
                },
                span: self.span(op_tok.span.start, close.span.end),
            });
            // `new Foo()` is a postfix primary: `new Map().set(...)`, `new Map().size`,
            // `new Foo()[i]` all continue the chain off the constructor result.
            return self.parse_postfix_from(new_expr);
        }
        let op = match self.peek().kind {
            TokenKind::Bang => UnOp::Not,
            TokenKind::Minus => UnOp::Neg,
            TokenKind::Plus => UnOp::Pos,
            _ => return self.parse_postfix(),
        };
        let op_tok = self.advance();
        let operand = self.parse_unary()?;
        let operand_span = self.ast.expr(operand).span;
        Some(self.ast.push_expr(Expr {
            kind: ExprKind::Unary { op, operand },
            span: self.span(op_tok.span.start, operand_span.end),
        }))
    }

    fn parse_postfix(&mut self) -> Option<ExprId> {
        let expr = self.parse_atom()?;
        self.parse_postfix_from(expr)
    }

    fn parse_postfix_from(&mut self, mut expr: ExprId) -> Option<ExprId> {
        // OptionalChain is emitted once at end-of-loop so a speculative-`<` rewind
        // never strands a half-built chain. Everything before the first `?.` accumulates
        // in `expr` normally; from the first `?.` onward ops go into `parts`.
        let mut chain_parts: Option<(ExprId, Vec<crate::ChainPart>)> = None;
        loop {
            match self.peek().kind {
                TokenKind::LeftParen => {
                    if let Some((_, parts)) = &mut chain_parts {
                        let part = self.parse_chain_call_tail(/*optional=*/ false)?;
                        parts.push(part);
                    } else {
                        expr = self.parse_call_tail(expr, None)?;
                    }
                }
                TokenKind::Dot => {
                    if let Some((_, parts)) = &mut chain_parts {
                        let part = self.parse_chain_field_tail(/*optional=*/ false)?;
                        parts.push(part);
                    } else {
                        expr = self.parse_field_access_tail(expr)?;
                    }
                }
                TokenKind::LeftBracket => {
                    if let Some((_, parts)) = &mut chain_parts {
                        let part = self.parse_chain_index_tail(/*optional=*/ false)?;
                        parts.push(part);
                    } else {
                        expr = self.parse_index_access_tail(expr)?;
                    }
                }
                TokenKind::QuestionDot => {
                    if chain_parts.is_none() {
                        chain_parts = Some((expr, Vec::new()));
                    }
                    self.advance();
                    let part = match self.peek().kind {
                        TokenKind::LeftParen => self.parse_chain_call_tail(true)?,
                        TokenKind::LeftBracket => self.parse_chain_index_tail(true)?,
                        _ if is_property_name(&self.peek().kind) => {
                            self.parse_chain_field_tail(true)?
                        }
                        _ => {
                            self.error_at_peek(
                                "expected field name, `(` for call, or `[` for index after `?.`",
                            );
                            return None;
                        }
                    };
                    if let Some((_, parts)) = &mut chain_parts {
                        parts.push(part);
                    }
                }
                // `<` after a callable expression might be a generic call `f<T>(args)`.
                // Speculatively parse; on failure restore state and let `parse_binary`
                // treat `<` as a comparison.
                TokenKind::LessThan => {
                    if chain_parts.is_some() {
                        // `a?.<T>()` not yet supported — terminate the chain.
                        break;
                    }
                    let saved_pos = self.pos;
                    let saved_diag_len = self.diagnostics.len();
                    if let Some(call_id) = self.try_parse_generic_call_tail(expr) {
                        expr = call_id;
                    } else {
                        self.pos = saved_pos;
                        self.diagnostics.truncate(saved_diag_len);
                        break;
                    }
                }
                TokenKind::Bang => {
                    if let Some((_, parts)) = &mut chain_parts {
                        let tok = self.advance();
                        parts.push(crate::ChainPart::NonNull { span: tok.span });
                        continue;
                    }
                    let tok = self.advance();
                    let start = self.ast.expr(expr).span.start;
                    expr = self.ast.push_expr(Expr {
                        kind: ExprKind::PostfixUnary {
                            op: PostfixOp::NonNullAssert,
                            operand: expr,
                        },
                        span: self.span(start, tok.span.end),
                    });
                }
                // Postfix `++`/`--` terminates the chain; `a?.b++` is rejected.
                TokenKind::PlusPlus | TokenKind::MinusMinus => {
                    if chain_parts.is_some() {
                        break;
                    }
                    let tok = self.advance();
                    let op = if matches!(tok.kind, TokenKind::PlusPlus) {
                        PostfixOp::Inc
                    } else {
                        PostfixOp::Dec
                    };
                    let start = self.ast.expr(expr).span.start;
                    expr = self.ast.push_expr(Expr {
                        kind: ExprKind::PostfixUnary { op, operand: expr },
                        span: self.span(start, tok.span.end),
                    });
                    break;
                }
                _ => break,
            }
        }
        if let Some((base, parts)) = chain_parts {
            let base_span = self.ast.expr(base).span;
            let end = parts.last().map_or(base_span.end, |p| p.span().end);
            Some(self.ast.push_expr(Expr {
                kind: ExprKind::OptionalChain { base, parts },
                span: self.span(base_span.start, end),
            }))
        } else {
            Some(expr)
        }
    }

    // `.` is consumed here for the non-optional case; `?.` was consumed by the caller.
    fn parse_chain_field_tail(&mut self, optional: bool) -> Option<crate::ChainPart> {
        let start_pos = self.peek().span.start;
        if !optional {
            self.advance();
        }
        let name_tok = if is_property_name(&self.peek().kind) {
            self.advance()
        } else {
            let what = if optional { "?." } else { "." };
            self.error_at_peek(format!("expected field name after `{what}`"));
            return None;
        };
        let name = self.ident_from_token(&name_tok);
        let end = name.span.end;
        Some(crate::ChainPart::Field {
            name,
            optional,
            span: self.span(start_pos, end),
        })
    }

    fn parse_chain_index_tail(&mut self, optional: bool) -> Option<crate::ChainPart> {
        let open = self.advance();
        let idx = self.parse_expression()?;
        if !matches!(self.peek().kind, TokenKind::RightBracket) {
            self.error_at_peek("expected `]`");
            return None;
        }
        let close = self.advance();
        Some(crate::ChainPart::Index {
            idx,
            optional,
            span: self.span(open.span.start, close.span.end),
        })
    }

    fn parse_chain_call_tail(&mut self, optional: bool) -> Option<crate::ChainPart> {
        let open = self.advance();
        let args = if matches!(self.peek().kind, TokenKind::RightParen) {
            Vec::new()
        } else {
            self.parse_call_args()?
        };
        if !matches!(self.peek().kind, TokenKind::RightParen) {
            self.error_at_peek("expected `)`");
            return None;
        }
        let close = self.advance();
        Some(crate::ChainPart::Call {
            args,
            type_args: None,
            optional,
            span: self.span(open.span.start, close.span.end),
        })
    }

    fn parse_field_access_tail(&mut self, receiver: ExprId) -> Option<ExprId> {
        self.advance();
        // Keywords are valid field names — `Foo.new(args)` is the canonical constructor call.
        let name_tok = if is_property_name(&self.peek().kind) {
            self.advance()
        } else {
            self.error_at_peek("expected field name after `.`");
            return None;
        };
        let name = self.ident_from_token(&name_tok);
        let receiver_span = self.ast.expr(receiver).span;
        Some(self.ast.push_expr(Expr {
            kind: ExprKind::FieldAccess {
                receiver,
                name: name.clone(),
            },
            span: self.span(receiver_span.start, name.span.end),
        }))
    }

    fn parse_index_access_tail(&mut self, receiver: ExprId) -> Option<ExprId> {
        self.advance();
        let index = self.parse_expression()?;
        if !matches!(self.peek().kind, TokenKind::RightBracket) {
            self.error_at_peek("expected `]`");
            return None;
        }
        let close = self.advance();
        let receiver_span = self.ast.expr(receiver).span;
        Some(self.ast.push_expr(Expr {
            kind: ExprKind::IndexAccess { receiver, index },
            span: self.span(receiver_span.start, close.span.end),
        }))
    }

    fn parse_call_tail(
        &mut self,
        callee: ExprId,
        type_args: Option<Vec<TypeAnnotation>>,
    ) -> Option<ExprId> {
        self.advance();
        let args = if matches!(self.peek().kind, TokenKind::RightParen) {
            Vec::new()
        } else {
            self.parse_call_args()?
        };
        if !matches!(self.peek().kind, TokenKind::RightParen) {
            self.error_at_peek("expected `)`");
            return None;
        }
        let close = self.advance();

        let callee_span = self.ast.expr(callee).span;
        Some(self.ast.push_expr(Expr {
            kind: ExprKind::Call {
                callee,
                type_args,
                args,
            },
            span: self.span(callee_span.start, close.span.end),
        }))
    }

    // Caller restores position + diagnostics on `None` so `<` is re-interpreted as comparison.
    fn try_parse_generic_call_tail(&mut self, callee: ExprId) -> Option<ExprId> {
        let type_args = self.try_parse_type_args_only()?;
        if !matches!(self.peek().kind, TokenKind::LeftParen) {
            return None;
        }
        self.parse_call_tail(callee, Some(type_args))
    }

    fn try_parse_type_args_only(&mut self) -> Option<Vec<TypeAnnotation>> {
        debug_assert!(matches!(self.peek().kind, TokenKind::LessThan));
        self.advance();

        if matches!(self.peek().kind, TokenKind::GreaterThan) {
            return None;
        }

        let mut type_args = Vec::new();
        loop {
            let ta = self.parse_type_annotation()?;
            type_args.push(ta);
            match self.peek().kind {
                TokenKind::Comma => {
                    self.advance();
                    if matches!(self.peek().kind, TokenKind::GreaterThan) {
                        self.advance();
                        break;
                    }
                }
                TokenKind::GreaterThan => {
                    self.advance();
                    break;
                }
                _ => return None,
            }
        }
        Some(type_args)
    }

    fn parse_call_args(&mut self) -> Option<Vec<ExprId>> {
        let mut args = Vec::new();
        loop {
            let arg = self.parse_expression()?;
            args.push(arg);
            match self.peek().kind {
                TokenKind::Comma => {
                    self.advance();
                    if matches!(self.peek().kind, TokenKind::RightParen) {
                        return Some(args);
                    }
                }
                TokenKind::RightParen => return Some(args),
                _ => {
                    self.error_at_peek("expected `,` or `)`");
                    return None;
                }
            }
        }
    }

    fn parse_atom(&mut self) -> Option<ExprId> {
        match self.peek().kind {
            TokenKind::LeftParen => return self.parse_paren(),
            // `{` here is an object literal; statement-start `{` is dispatched to
            // `parse_block` before `parse_atom` runs, so the two stay disjoint.
            TokenKind::LeftBrace => return self.parse_object_literal(),
            TokenKind::LeftBracket => return self.parse_array_literal(),
            TokenKind::TemplateHead(_) => return self.parse_template_literal(),
            TokenKind::This => return self.parse_this_or_super(true),
            TokenKind::Super => return self.parse_this_or_super(false),
            TokenKind::Function => return self.parse_function_expression(),
            _ => {}
        }
        if !matches!(
            self.peek().kind,
            TokenKind::NumberLiteral(_)
                | TokenKind::BigIntLiteral(_)
                | TokenKind::StringLiteral(_)
                | TokenKind::TemplateNoSubstitution(_)
                | TokenKind::BooleanLiteral(_)
                | TokenKind::NullLiteral
                | TokenKind::Identifier
                | TokenKind::RegexLiteral { .. }
        ) {
            self.error_at_peek("expected expression");
            return None;
        }
        let tok = self.advance();
        let span = tok.span;
        let kind = match tok.kind {
            TokenKind::NumberLiteral(v) => ExprKind::Number(v),
            TokenKind::BigIntLiteral(s) => ExprKind::BigInt(s),
            TokenKind::StringLiteral(s) => ExprKind::String(s),
            TokenKind::TemplateNoSubstitution(s) => ExprKind::String(s),
            TokenKind::BooleanLiteral(b) => ExprKind::Boolean(b),
            TokenKind::NullLiteral => ExprKind::Null,
            TokenKind::Identifier => ExprKind::Identifier(Ident {
                name: self.source[span.start as usize..span.end as usize].to_string(),
                span,
            }),
            TokenKind::RegexLiteral { source, flags } => ExprKind::Regex { source, flags },
            _ => unreachable!("dispatch above already filtered"),
        };
        Some(self.ast.push_expr(Expr { kind, span }))
    }

    /// Parses `this` / `super` as primary expressions. Both are gated to class
    /// method/constructor bodies via `class_member_body_depth`. The depth is lexical
    /// and does not model a nested non-arrow `function` rebinding `this` — the
    /// typechecker performs the precise binding (deferred to the class typechecking work).
    fn parse_this_or_super(&mut self, is_this: bool) -> Option<ExprId> {
        let tok = self.advance();
        let span = tok.span;
        if self.class_member_body_depth == 0 {
            if is_this {
                self.error_at_with_help(
                    span,
                    "`this` is only valid inside a class method or constructor body",
                    vec![
                        "reference `this` from within a class method or `constructor`".to_string(),
                    ],
                );
            } else {
                self.error_at_with_help(
                    span,
                    "`super` is only valid inside a class method or constructor body",
                    vec![
                        "call `super(...)` in a subclass constructor or `super.method(...)` \
                         in a subclass method"
                            .to_string(),
                    ],
                );
            }
        }
        let kind = if is_this {
            ExprKind::This
        } else {
            ExprKind::Super
        };
        Some(self.ast.push_expr(Expr { kind, span }))
    }

    // The lexer guarantees: TemplateHead → expr → (TemplateMiddle → expr)* → TemplateTail.
    fn parse_template_literal(&mut self) -> Option<ExprId> {
        let head_tok = self.advance();
        let TokenKind::TemplateHead(head) = head_tok.kind else {
            unreachable!("dispatch guaranteed TemplateHead");
        };
        let start = head_tok.span.start;
        let mut parts: Vec<String> = vec![head];
        let mut exprs: Vec<ExprId> = Vec::new();
        loop {
            let expr = self.parse_expression()?;
            exprs.push(expr);
            match self.peek().kind.clone() {
                TokenKind::TemplateMiddle(s) => {
                    self.advance();
                    parts.push(s);
                }
                TokenKind::TemplateTail(s) => {
                    let tok = self.advance();
                    parts.push(s);
                    return Some(self.ast.push_expr(Expr {
                        kind: ExprKind::TemplateLiteral { parts, exprs },
                        span: self.span(start, tok.span.end),
                    }));
                }
                _ => {
                    self.error_at_peek("expected `}` to close template interpolation");
                    return None;
                }
            }
        }
    }

    fn parse_object_literal(&mut self) -> Option<ExprId> {
        let open = self.advance();

        let mut members: Vec<ObjectLiteralMember> = Vec::new();
        if !matches!(self.peek().kind, TokenKind::RightBrace) {
            loop {
                if matches!(self.peek().kind, TokenKind::DotDotDot) {
                    let dots = self.advance();
                    let value = self.parse_expression()?;
                    let value_span = self.ast.expr(value).span;
                    members.push(ObjectLiteralMember::Spread {
                        value,
                        span: self.span(dots.span.start, value_span.end),
                    });
                } else {
                    let field = self.parse_object_literal_field()?;
                    // Spread members are dynamic — only literal fields participate in
                    // the duplicate-key check.
                    if let Some(existing) = members.iter().find_map(|m| match m {
                        ObjectLiteralMember::Field(f) if f.name.name == field.name.name => Some(f),
                        _ => None,
                    }) {
                        self.diagnostics.push(Diagnostic {
                            severity: Severity::Error,
                            span: field.name.span,
                            message: format!(
                                "duplicate field `{}` in object literal",
                                field.name.name
                            ),
                            help: vec![],
                            notes: vec![(existing.name.span, "first defined here".into())],
                        });
                        // Keep parsing so the user sees all duplicates at once.
                    }
                    members.push(ObjectLiteralMember::Field(field));
                }
                match self.peek().kind {
                    TokenKind::Comma => {
                        self.advance();
                        if matches!(self.peek().kind, TokenKind::RightBrace) {
                            break;
                        }
                    }
                    TokenKind::RightBrace => break,
                    _ => {
                        self.error_at_peek("expected `,` or `}`");
                        return None;
                    }
                }
            }
        }

        if !matches!(self.peek().kind, TokenKind::RightBrace) {
            self.error_at_peek("expected `}`");
            return None;
        }
        let close = self.advance();

        Some(self.ast.push_expr(Expr {
            kind: ExprKind::ObjectLiteral { members },
            span: self.span(open.span.start, close.span.end),
        }))
    }

    fn parse_object_literal_field(&mut self) -> Option<ObjectLiteralField> {
        let (name, shorthandable) = match self.peek().kind.clone() {
            TokenKind::Identifier => {
                let tok = self.advance();
                (self.ident_from_token(&tok), true)
            }
            kind if is_property_name(&kind) => {
                let tok = self.advance();
                (self.ident_from_token(&tok), false)
            }
            TokenKind::StringLiteral(_) => {
                let tok = self.advance();
                (self.property_ident_from_token(&tok), false)
            }
            _ => {
                self.error_at_peek("expected field name");
                return None;
            }
        };

        // `{ m(x: T): R { … } }` is shorthand for `{ m: (x: T): R => { … } }`. Method
        // shorthand carries no `this` of its own here, for the same reason a function
        // expression does not, so the arrow is an exact lowering. Checked before the
        // `:` branch and independently of `shorthandable`, because a keyword or string
        // key can name a method too.
        if matches!(self.peek().kind, TokenKind::LeftParen) {
            let value = self.parse_method_shorthand_body(name.span)?;
            return Some(ObjectLiteralField { name, value });
        }

        if !matches!(self.peek().kind, TokenKind::Colon) {
            // `{ x }` is shorthand for `{ x: x }`. Only identifier keys can use
            // it — a string key like `{ "x" }` has no binding to reference.
            if shorthandable {
                let value = self.ast.push_expr(Expr {
                    kind: ExprKind::Identifier(name.clone()),
                    span: name.span,
                });
                return Some(ObjectLiteralField { name, value });
            }
            self.error_at_peek("expected `:` after field name");
            return None;
        }
        self.advance();

        let value = self.parse_expression()?;
        Some(ObjectLiteralField { name, value })
    }

    /// The `(params): R { … }` tail of an object-literal method, lowered to an arrow.
    /// `name_span` is the key's span, so the arrow spans the whole member.
    fn parse_method_shorthand_body(&mut self, name_span: Span) -> Option<ExprId> {
        self.advance();

        let params = if matches!(self.peek().kind, TokenKind::RightParen) {
            Vec::new()
        } else {
            self.parse_arrow_param_list()?
        };
        if !matches!(self.peek().kind, TokenKind::RightParen) {
            self.error_at_peek("expected `)`");
            return None;
        }
        self.advance();

        let (return_type, type_predicate) = if matches!(self.peek().kind, TokenKind::Colon) {
            self.advance();
            self.parse_predicate_or_return_type(TypePos::ArrowReturn)?
        } else {
            (None, None)
        };

        if !matches!(self.peek().kind, TokenKind::LeftBrace) {
            self.error_at_peek("expected `{` to open the method body");
            return None;
        }
        let block = self.parse_outer_this_boundary(Self::parse_block)?;
        let end = self.ast.stmt(block).span.end;

        Some(self.ast.push_expr(Expr {
            kind: ExprKind::Arrow {
                params,
                return_type,
                type_predicate,
                body: ArrowBody::Block(block),
            },
            span: self.span(name_span.start, end),
        }))
    }

    fn parse_array_literal(&mut self) -> Option<ExprId> {
        let open = self.advance();

        let mut elements: Vec<ArrayLiteralElement> = Vec::new();
        if !matches!(self.peek().kind, TokenKind::RightBracket) {
            loop {
                let element = if matches!(self.peek().kind, TokenKind::DotDotDot) {
                    let dots = self.advance();
                    let value = self.parse_expression()?;
                    let value_span = self.ast.expr(value).span;
                    ArrayLiteralElement::Spread {
                        value,
                        span: self.span(dots.span.start, value_span.end),
                    }
                } else {
                    ArrayLiteralElement::Value(self.parse_expression()?)
                };
                elements.push(element);
                match self.peek().kind {
                    TokenKind::Comma => {
                        self.advance();
                        if matches!(self.peek().kind, TokenKind::RightBracket) {
                            break;
                        }
                    }
                    TokenKind::RightBracket => break,
                    _ => {
                        self.error_at_peek("expected `,` or `]`");
                        return None;
                    }
                }
            }
        }

        if !matches!(self.peek().kind, TokenKind::RightBracket) {
            self.error_at_peek("expected `]`");
            return None;
        }
        let close = self.advance();

        Some(self.ast.push_expr(Expr {
            kind: ExprKind::ArrayLiteral { elements },
            span: self.span(open.span.start, close.span.end),
        }))
    }

    fn parse_paren(&mut self) -> Option<ExprId> {
        let open = self.advance();
        let inner = self.parse_expression()?;
        if !matches!(self.peek().kind, TokenKind::RightParen) {
            self.error_at_peek("expected `)`");
            return None;
        }
        let close = self.advance();
        let span = self.span(open.span.start, close.span.end);
        Some(self.ast.push_expr(Expr {
            kind: ExprKind::Paren(inner),
            span,
        }))
    }

    fn recover(&mut self) {
        while !self.is_at_eof() {
            match self.peek().kind {
                TokenKind::Semicolon => {
                    self.advance();
                    return;
                }
                TokenKind::RightBrace
                | TokenKind::Let
                | TokenKind::Const
                | TokenKind::Function
                | TokenKind::Export
                | TokenKind::If
                | TokenKind::While
                | TokenKind::Return
                | TokenKind::Try
                | TokenKind::Throw => return,
                _ => {
                    self.advance();
                }
            }
        }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn peek_at(&self, offset: usize) -> &Token {
        let idx = (self.pos + offset).min(self.tokens.len() - 1);
        &self.tokens[idx]
    }

    fn advance(&mut self) -> Token {
        let tok = self.tokens[self.pos].clone();
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn is_at_eof(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Eof)
    }

    /// End offset of the most recently consumed token (the body of the file's first
    /// token if nothing has been consumed yet).
    fn prev_token_end(&self) -> u32 {
        self.tokens[self.pos.saturating_sub(1)].span.end
    }

    fn take_leading_doc(&self) -> Option<crate::DocComment> {
        self.peek()
            .leading_doc
            .as_ref()
            .map(crate::parse_doc_comment)
    }

    fn error_at(&mut self, span: Span, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            severity: Severity::Error,
            span,
            message: message.into(),
            help: vec![],
            notes: vec![],
        });
    }

    fn error_at_with_help(&mut self, span: Span, message: impl Into<String>, help: Vec<String>) {
        self.diagnostics.push(Diagnostic {
            severity: Severity::Error,
            span,
            message: message.into(),
            help,
            notes: vec![],
        });
    }

    fn error_at_peek(&mut self, message: impl Into<String>) {
        let span = self.peek().span;
        self.error_at(span, message);
    }

    fn error_at_peek_with_help(&mut self, message: impl Into<String>, help: Vec<String>) {
        let span = self.peek().span;
        self.error_at_with_help(span, message, help);
    }

    fn error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
    }
}

fn is_property_name(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Identifier
            | TokenKind::BooleanLiteral(_)
            | TokenKind::NullLiteral
            | TokenKind::Let
            | TokenKind::Const
            | TokenKind::Function
            | TokenKind::If
            | TokenKind::Else
            | TokenKind::While
            | TokenKind::Do
            | TokenKind::For
            | TokenKind::Break
            | TokenKind::Continue
            | TokenKind::Return
            | TokenKind::Switch
            | TokenKind::Case
            | TokenKind::Default
            | TokenKind::Void
            | TokenKind::Interface
            | TokenKind::Enum
            | TokenKind::In
            | TokenKind::Typeof
            | TokenKind::Import
            | TokenKind::Export
            | TokenKind::New
            | TokenKind::Try
            | TokenKind::Catch
            | TokenKind::Finally
            | TokenKind::Throw
            | TokenKind::Class
            | TokenKind::Extends
            | TokenKind::Implements
            | TokenKind::Super
            | TokenKind::This
    )
}

fn is_reserved_identifier_word(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::BooleanLiteral(_)
            | TokenKind::NullLiteral
            | TokenKind::Let
            | TokenKind::Const
            | TokenKind::Function
            | TokenKind::If
            | TokenKind::Else
            | TokenKind::While
            | TokenKind::Do
            | TokenKind::For
            | TokenKind::Break
            | TokenKind::Continue
            | TokenKind::Return
            | TokenKind::Switch
            | TokenKind::Case
            | TokenKind::Default
            | TokenKind::Export
            | TokenKind::Void
            | TokenKind::Interface
            | TokenKind::Enum
            | TokenKind::In
            | TokenKind::Typeof
            | TokenKind::Import
            | TokenKind::New
            | TokenKind::Try
            | TokenKind::Catch
            | TokenKind::Finally
            | TokenKind::Throw
            | TokenKind::Class
            | TokenKind::Extends
            | TokenKind::Implements
            | TokenKind::Super
            | TokenKind::This
    )
}

fn reserved_keyword_rename_example(keyword: &str) -> &'static str {
    match keyword {
        "default" => "defaultValue",
        _ => "valueName",
    }
}

fn mixed_logical(prev: BinOp, next: BinOp) -> bool {
    let prev_logical = matches!(prev, BinOp::Or | BinOp::And);
    let next_logical = matches!(next, BinOp::Or | BinOp::And);
    let prev_nullish = matches!(prev, BinOp::NullishCoalesce);
    let next_nullish = matches!(next, BinOp::NullishCoalesce);
    (prev_logical && next_nullish) || (prev_nullish && next_logical)
}

fn peek_binop(kind: &TokenKind) -> Option<(BinOp, u8)> {
    Some(match kind {
        TokenKind::QuestionQuestion => (BinOp::NullishCoalesce, 0),
        TokenKind::PipePipe => (BinOp::Or, 1),
        TokenKind::AmpAmp => (BinOp::And, 2),
        TokenKind::EqEqEq | TokenKind::EqEq => (BinOp::Eq, 3),
        TokenKind::BangEqEq | TokenKind::BangEq => (BinOp::NotEq, 3),
        TokenKind::LessThan => (BinOp::Lt, 4),
        TokenKind::GreaterThan => (BinOp::Gt, 4),
        TokenKind::LessEquals => (BinOp::Le, 4),
        TokenKind::GreaterEquals => (BinOp::Ge, 4),
        TokenKind::In => (BinOp::In, 4),
        TokenKind::Plus => (BinOp::Add, 5),
        TokenKind::Minus => (BinOp::Sub, 5),
        TokenKind::Star => (BinOp::Mul, 6),
        TokenKind::Slash => (BinOp::Div, 6),
        TokenKind::Percent => (BinOp::Rem, 6),
        // `**` recurses with `prec` not `prec + 1` — that's what makes it right-associative.
        TokenKind::StarStar => (BinOp::Pow, 7),
        _ => return None,
    })
}

fn is_right_associative(op: BinOp) -> bool {
    matches!(op, BinOp::Pow)
}

fn compound_op_for_token(kind: &TokenKind) -> Option<BinOp> {
    Some(match kind {
        TokenKind::PlusEquals => BinOp::Add,
        TokenKind::MinusEquals => BinOp::Sub,
        TokenKind::StarEquals => BinOp::Mul,
        TokenKind::SlashEquals => BinOp::Div,
        TokenKind::PercentEquals => BinOp::Rem,
        TokenKind::StarStarEquals => BinOp::Pow,
        _ => return None,
    })
}

fn is_assign_lookahead(kind: &TokenKind) -> bool {
    matches!(kind, TokenKind::Equals) || compound_op_for_token(kind).is_some()
}

#[cfg(test)]
mod tests {
    use super::{MAX_ERRORS, Parser, parse};
    use crate::source::Sources;
    use crate::{Asi, Ast, ExprKind, FileId, ImportKind, StmtKind, Token, TokenKind, diagnostics};

    const F: FileId = FileId(0);

    fn tokens_of(source: &str) -> Vec<Token> {
        let mut asi = Asi::new(source, crate::FileId(0));
        let mut tokens = Vec::new();
        loop {
            let tok = asi.next_token();
            let is_eof = matches!(tok.kind, TokenKind::Eof);
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        let diags = asi.into_diagnostics();
        assert!(diags.is_empty(), "unexpected lexer diagnostics: {diags:?}");
        tokens
    }

    fn parse_str(source: &str) -> (Ast, Vec<crate::Diagnostic>) {
        parse(source, tokens_of(source), crate::FileId(0))
    }

    fn parser_from_source(source: &str) -> Parser<'_> {
        Parser {
            source,
            tokens: tokens_of(source),
            file: F,
            pos: 0,
            ast: Ast::new(),
            diagnostics: Vec::new(),
            block_depth: 0,
            class_member_body_depth: 0,
        }
    }

    #[test]
    fn parse_empty_input() {
        let (ast, diags) = parse_str("");
        assert!(diags.is_empty());
        assert!(ast.top_level.is_empty());
    }

    #[test]
    fn any_type_is_rejected() {
        // `any` is not supported — the parser rejects it in every type position
        // (annotation, cast target, array/generic element) and points to the fix.
        for src in [
            "function main(): void { const x: any = 1; }",
            "function main(): void { const x = 1 as any; }",
            "function main(): void { const x: any[] = []; }",
        ] {
            let (_ast, diags) = parse_str(src);
            assert!(
                diags
                    .iter()
                    .any(|d| d.message.contains("`any` is not supported")),
                "expected an `any` rejection for {src:?}, got: {diags:?}",
            );
            assert!(
                diags
                    .iter()
                    .any(|d| d.help.iter().any(|h| h.contains("`unknown`"))),
                "expected the fix-shape help for {src:?}, got: {diags:?}",
            );
        }
    }

    #[test]
    fn arrow_return_type_scanner_matches_type_grammar() {
        // Every form the annotation grammar accepts must also be accepted by
        // the arrow-disambiguator's lookahead (scan_past_type_annotation):
        // a miss there turns a valid arrow into `expected expression`.
        // When adding a type form to parse_type_array, add a row here.
        for ty in [
            "number",
            "void",
            "null",
            "ns.Foo",
            "Map<string, number>",
            "ns.Foo<string>[]",
            "number[]",
            "[number, number]",
            "[number, [string, boolean]]",
            "[number, string][]",
            "{ a: number }",
            "{ a: number }[]",
            "\"on\" | \"off\"",
            "1 | 2",
            "Point | null",
            "| number | string",
            "(n: number) => number",
            "(n: number) => [number, number]",
        ] {
            let annotation = format!("function f(): void {{ const a: {ty} = x; }}");
            let (_ast, diags) = parse_str(&annotation);
            assert!(
                diags.is_empty(),
                "annotation grammar rejected {ty:?}: {diags:?}",
            );

            let arrow = format!("function f(): void {{ const g = (): {ty} => x; }}");
            let (_ast, diags) = parse_str(&arrow);
            assert!(
                diags.is_empty(),
                "arrow lookahead rejected return type {ty:?}: {diags:?}",
            );
        }

        // Type-predicate returns ride the same scanner.
        let (_ast, diags) =
            parse_str("function f(): void { const g = (v: unknown): v is string => x; }");
        assert!(
            diags.is_empty(),
            "arrow lookahead rejected predicate return: {diags:?}",
        );
    }

    #[test]
    fn bare_semicolons_are_silent_empty_statements() {
        let (ast, diags) = parse_str(";");
        assert!(diags.is_empty());
        assert!(ast.top_level.is_empty());
    }

    #[test]
    fn parse_single_non_expression_token_errors() {
        let (ast, diags) = parse_str("}");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "expected expression");
        assert!(ast.top_level.is_empty());
    }

    #[test]
    fn recovery_across_many_bad_starts() {
        let (ast, diags) = parse_str("} } } x;");
        assert_eq!(diags.len(), 3);
        for d in &diags {
            assert_eq!(d.message, "expected expression");
        }
        assert_eq!(ast.top_level.len(), 1);
    }

    #[test]
    fn recovery_consumes_semicolon() {
        let mut p = parser_from_source("foo ;");
        p.error_at_peek("test");
        p.recover();
        assert!(matches!(
            p.peek().kind,
            TokenKind::Semicolon | TokenKind::Eof
        ));
        assert!(matches!(p.peek().kind, TokenKind::Eof));
    }

    #[test]
    fn recovery_stops_at_right_brace() {
        let mut p = parser_from_source("foo }");
        p.error_at_peek("test");
        p.recover();
        assert!(matches!(p.peek().kind, TokenKind::RightBrace));
    }

    #[test]
    fn recovery_stops_at_let_keyword() {
        let mut p = parser_from_source("foo let x");
        p.error_at_peek("test");
        p.recover();
        assert!(matches!(p.peek().kind, TokenKind::Let));
    }

    #[test]
    fn recovery_stops_at_function_keyword() {
        let mut p = parser_from_source("foo function bar");
        p.error_at_peek("test");
        p.recover();
        assert!(matches!(p.peek().kind, TokenKind::Function));
    }

    #[test]
    fn error_cap_at_20() {
        let source = "} ".repeat(25);
        let (_, diags) = parse_str(&source);
        assert_eq!(diags.len(), MAX_ERRORS);
    }

    #[test]
    fn driver_makes_progress_past_unmatched_brace() {
        let (ast, diags) = parse_str("} x;");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "expected expression");
        assert_eq!(ast.top_level.len(), 1);
    }

    #[test]
    fn snapshot_multi_error_render() {
        let source = "foo bar;\nbaz qux;\nhello world;";
        let (_, diags) = parse_str(source);
        let (sources, _) = Sources::single("script.subm", source);
        let rendered: String = diags
            .iter()
            .map(|d| diagnostics::render(d, &sources))
            .collect::<Vec<_>>()
            .join("\n");
        insta::assert_snapshot!(rendered);
    }

    fn single_stmt(ast: &Ast) -> &crate::Stmt {
        assert_eq!(ast.top_level.len(), 1);
        ast.stmt(ast.top_level[0])
    }

    fn expr_of_single_stmt(ast: &Ast) -> &crate::Expr {
        let stmt = single_stmt(ast);
        match stmt.kind {
            crate::StmtKind::Expr(id) => ast.expr(id),
            _ => panic!("expected expression statement, got {:?}", stmt.kind),
        }
    }

    #[test]
    fn parse_number_literal() {
        let (ast, diags) = parse_str("42;");
        assert!(diags.is_empty());
        let expr = expr_of_single_stmt(&ast);
        assert_eq!(expr.kind, crate::ExprKind::Number(42.0));
        assert_eq!(expr.span, crate::Span::new(F, 0, 2));
    }

    #[test]
    fn parse_string_literal() {
        let (ast, diags) = parse_str(r#""s";"#);
        assert!(diags.is_empty());
        let expr = expr_of_single_stmt(&ast);
        assert_eq!(expr.kind, crate::ExprKind::String("s".to_string()));
        assert_eq!(expr.span, crate::Span::new(F, 0, 3));
    }

    #[test]
    fn parse_boolean_true() {
        let (ast, diags) = parse_str("true;");
        assert!(diags.is_empty());
        let expr = expr_of_single_stmt(&ast);
        assert_eq!(expr.kind, crate::ExprKind::Boolean(true));
        assert_eq!(expr.span, crate::Span::new(F, 0, 4));
    }

    #[test]
    fn parse_boolean_false() {
        let (ast, diags) = parse_str("false;");
        assert!(diags.is_empty());
        let expr = expr_of_single_stmt(&ast);
        assert_eq!(expr.kind, crate::ExprKind::Boolean(false));
        assert_eq!(expr.span, crate::Span::new(F, 0, 5));
    }

    #[test]
    fn parse_null_literal() {
        let (ast, diags) = parse_str("null;");
        assert!(diags.is_empty());
        let expr = expr_of_single_stmt(&ast);
        assert_eq!(expr.kind, crate::ExprKind::Null);
        assert_eq!(expr.span, crate::Span::new(F, 0, 4));
    }

    #[test]
    fn parse_identifier() {
        let (ast, diags) = parse_str("x;");
        assert!(diags.is_empty());
        let expr = expr_of_single_stmt(&ast);
        assert!(matches!(expr.kind, crate::ExprKind::Identifier(_)));
        assert_eq!(expr.span, crate::Span::new(F, 0, 1));
    }

    #[test]
    fn reserved_keywords_remain_invalid_declaration_names() {
        let (_ast, diags) = parse_str("function default(): void {}");
        assert_eq!(diags.len(), 1, "{diags:#?}");
        assert_eq!(
            diags[0].message,
            "`default` is a reserved keyword and can't be used as a name"
        );
        assert!(
            diags[0]
                .help
                .iter()
                .any(|h| h == "rename it, for example: `defaultValue`"),
            "expected rename help, got: {diags:?}"
        );
    }

    #[test]
    fn reserved_keywords_remain_invalid_parameter_names() {
        let (_ast, diags) = parse_str("function f(default: string): void {}");
        assert_eq!(diags.len(), 1, "{diags:#?}");
        assert_eq!(
            diags[0].message,
            "`default` is a reserved keyword and can't be used as a name"
        );
        assert!(
            diags[0]
                .help
                .iter()
                .any(|h| h == "rename it, for example: `defaultValue`"),
            "expected rename help, got: {diags:?}"
        );
    }

    #[test]
    fn contextual_keywords_are_valid_names() {
        for src in [
            "const type = \"x\";",
            "const from = 1;",
            "let of = 2;",
            "const as = 3;",
            "const is = 4;",
            "function f(from: string): void {}",
        ] {
            let (_ast, diags) = parse_str(src);
            assert!(
                diags.is_empty(),
                "unexpected diagnostics for {src:?}: {diags:#?}"
            );
        }
    }

    #[test]
    fn type_alias_dispatch_table() {
        let (ast, diags) = parse_str("type X = number;");
        assert!(diags.is_empty(), "{diags:#?}");
        assert!(matches!(
            ast.stmt(ast.top_level[0]).kind,
            crate::StmtKind::TypeAliasDecl { .. }
        ));

        // A binding named `type` keeps expression/assignment statements working.
        for src in ["type = 5;", "type;", "type(1);", "type < 3;", "type.foo;"] {
            let (ast, diags) = parse_str(src);
            assert!(
                diags.is_empty(),
                "unexpected diagnostics for {src:?}: {diags:#?}"
            );
            assert!(
                !matches!(
                    ast.stmt(ast.top_level[0]).kind,
                    crate::StmtKind::TypeAliasDecl { .. }
                ),
                "{src:?} must not parse as a type alias"
            );
        }

        let (_ast, diags) = parse_str("type X;");
        assert!(
            diags
                .iter()
                .any(|d| d.message == "expected `=` after type alias name"),
            "bare `type X` should commit to the alias diagnostic, got: {diags:#?}"
        );
    }

    #[test]
    fn export_type_alias_dispatch() {
        let (ast, diags) = parse_str("export type X = number;");
        assert!(diags.is_empty(), "{diags:#?}");
        assert_eq!(ast.exported_decls.len(), 1);
        assert!(matches!(
            ast.stmt(ast.exported_decls[0].stmt).kind,
            crate::StmtKind::TypeAliasDecl { .. }
        ));

        let (_ast, diags) = parse_str("export type = 5;");
        assert!(
            diags
                .iter()
                .any(|d| d.message == "expected a declaration after `export`"),
            "`export type = 5` must not become an exported assignment, got: {diags:#?}"
        );
    }

    #[test]
    fn parse_paren_simple() {
        let (ast, diags) = parse_str("(x);");
        assert!(diags.is_empty());
        let expr = expr_of_single_stmt(&ast);
        let crate::ExprKind::Paren(inner_id) = expr.kind else {
            panic!("expected Paren, got {:?}", expr.kind);
        };
        assert_eq!(expr.span, crate::Span::new(F, 0, 3));
        let inner = ast.expr(inner_id);
        assert!(matches!(inner.kind, crate::ExprKind::Identifier(_)));
        assert_eq!(inner.span, crate::Span::new(F, 1, 2));
    }

    #[test]
    fn parse_paren_nested() {
        let (ast, diags) = parse_str("((x));");
        assert!(diags.is_empty());
        let outer = expr_of_single_stmt(&ast);
        let crate::ExprKind::Paren(mid_id) = outer.kind else {
            panic!("expected outer Paren");
        };
        assert_eq!(outer.span, crate::Span::new(F, 0, 5));
        let mid = ast.expr(mid_id);
        let crate::ExprKind::Paren(inner_id) = mid.kind else {
            panic!("expected inner Paren");
        };
        assert_eq!(mid.span, crate::Span::new(F, 1, 4));
        let inner = ast.expr(inner_id);
        assert!(matches!(inner.kind, crate::ExprKind::Identifier(_)));
        assert_eq!(inner.span, crate::Span::new(F, 2, 3));
    }

    #[test]
    fn parse_sequence_of_atoms() {
        let (ast, diags) = parse_str(r#"42; "hi"; true; null; x; (x);"#);
        assert!(diags.is_empty());
        assert_eq!(ast.top_level.len(), 6);
        insta::assert_debug_snapshot!(ast);
    }

    #[test]
    fn missing_semicolon_after_expression() {
        let (ast, diags) = parse_str("42 43");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "expected `;` after expression");
        assert!(ast.top_level.is_empty());
    }

    #[test]
    fn empty_paren_diagnoses() {
        let (ast, diags) = parse_str("();");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "expected expression");
        assert!(ast.top_level.is_empty());
    }

    #[test]
    fn unterminated_paren_diagnoses() {
        let (ast, diags) = parse_str("(x;");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "expected `)`");
        assert!(ast.top_level.is_empty());
    }

    fn binary(e: &crate::Expr) -> (crate::BinOp, crate::ExprId, crate::ExprId) {
        match e.kind {
            crate::ExprKind::Binary { op, lhs, rhs } => (op, lhs, rhs),
            _ => panic!("expected Binary, got {:?}", e.kind),
        }
    }

    #[test]
    fn parse_binary_add() {
        let (ast, diags) = parse_str("a + b;");
        assert!(diags.is_empty());
        let outer = expr_of_single_stmt(&ast);
        let (op, lhs, rhs) = binary(outer);
        assert_eq!(op, crate::BinOp::Add);
        assert!(matches!(ast.expr(lhs).kind, crate::ExprKind::Identifier(_)));
        assert!(matches!(ast.expr(rhs).kind, crate::ExprKind::Identifier(_)));
        assert_eq!(outer.span, crate::Span::new(F, 0, 5));
    }

    #[test]
    fn precedence_mul_binds_tighter_than_add() {
        let (ast, diags) = parse_str("a + b * c;");
        assert!(diags.is_empty());
        let outer = expr_of_single_stmt(&ast);
        let (op, lhs, rhs) = binary(outer);
        assert_eq!(op, crate::BinOp::Add);
        assert!(matches!(ast.expr(lhs).kind, crate::ExprKind::Identifier(_)));
        let (rop, rlhs, rrhs) = binary(ast.expr(rhs));
        assert_eq!(rop, crate::BinOp::Mul);
        assert!(matches!(
            ast.expr(rlhs).kind,
            crate::ExprKind::Identifier(_)
        ));
        assert!(matches!(
            ast.expr(rrhs).kind,
            crate::ExprKind::Identifier(_)
        ));
    }

    #[test]
    fn precedence_mul_left_of_add() {
        let (ast, diags) = parse_str("a * b + c;");
        assert!(diags.is_empty());
        let outer = expr_of_single_stmt(&ast);
        let (op, lhs, rhs) = binary(outer);
        assert_eq!(op, crate::BinOp::Add);
        let (lop, ..) = binary(ast.expr(lhs));
        assert_eq!(lop, crate::BinOp::Mul);
        assert!(matches!(ast.expr(rhs).kind, crate::ExprKind::Identifier(_)));
    }

    #[test]
    fn left_associative_same_precedence() {
        let (ast, diags) = parse_str("a - b - c;");
        assert!(diags.is_empty());
        let outer = expr_of_single_stmt(&ast);
        let (op, lhs, rhs) = binary(outer);
        assert_eq!(op, crate::BinOp::Sub);
        let (lop, llhs, lrhs) = binary(ast.expr(lhs));
        assert_eq!(lop, crate::BinOp::Sub);
        assert!(matches!(
            ast.expr(llhs).kind,
            crate::ExprKind::Identifier(_)
        ));
        assert!(matches!(
            ast.expr(lrhs).kind,
            crate::ExprKind::Identifier(_)
        ));
        assert!(matches!(ast.expr(rhs).kind, crate::ExprKind::Identifier(_)));
    }

    #[test]
    fn equality_inside_logical() {
        let (ast, diags) = parse_str("a === b && c === d;");
        assert!(diags.is_empty());
        let outer = expr_of_single_stmt(&ast);
        let (op, lhs, rhs) = binary(outer);
        assert_eq!(op, crate::BinOp::And);
        let (lop, ..) = binary(ast.expr(lhs));
        assert_eq!(lop, crate::BinOp::Eq);
        let (rop, ..) = binary(ast.expr(rhs));
        assert_eq!(rop, crate::BinOp::Eq);
    }

    #[test]
    fn double_equal_is_same_variant_as_triple() {
        let (ast, _) = parse_str("a == b;");
        let outer = expr_of_single_stmt(&ast);
        let (op, ..) = binary(outer);
        assert_eq!(op, crate::BinOp::Eq);
    }

    #[test]
    fn bang_eq_is_not_eq() {
        let (ast, _) = parse_str("a != b;");
        let outer = expr_of_single_stmt(&ast);
        let (op, ..) = binary(outer);
        assert_eq!(op, crate::BinOp::NotEq);
    }

    #[test]
    fn comparison_has_higher_prec_than_logical() {
        let (ast, _) = parse_str("a < b && c;");
        let outer = expr_of_single_stmt(&ast);
        let (op, lhs, _) = binary(outer);
        assert_eq!(op, crate::BinOp::And);
        let (lop, ..) = binary(ast.expr(lhs));
        assert_eq!(lop, crate::BinOp::Lt);
    }

    #[test]
    fn full_precedence_chain_snapshot() {
        let (ast, diags) = parse_str("a || b && c === d < e + f * g;");
        assert!(diags.is_empty());
        insta::assert_debug_snapshot!(ast);
    }

    #[test]
    fn paren_overrides_precedence() {
        let (ast, _) = parse_str("(a + b) * c;");
        let outer = expr_of_single_stmt(&ast);
        let (op, lhs, rhs) = binary(outer);
        assert_eq!(op, crate::BinOp::Mul);
        let crate::ExprKind::Paren(inner_id) = ast.expr(lhs).kind else {
            panic!("expected Paren on lhs");
        };
        let (inner_op, ..) = binary(ast.expr(inner_id));
        assert_eq!(inner_op, crate::BinOp::Add);
        assert!(matches!(ast.expr(rhs).kind, crate::ExprKind::Identifier(_)));
    }

    #[test]
    fn binary_missing_rhs_diagnoses() {
        let (ast, diags) = parse_str("a +;");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "expected expression");
        assert!(ast.top_level.is_empty());
    }

    fn unary(e: &crate::Expr) -> (crate::UnOp, crate::ExprId) {
        match e.kind {
            crate::ExprKind::Unary { op, operand } => (op, operand),
            _ => panic!("expected Unary, got {:?}", e.kind),
        }
    }

    #[test]
    fn parse_unary_not() {
        let (ast, _) = parse_str("!x;");
        let outer = expr_of_single_stmt(&ast);
        let (op, operand) = unary(outer);
        assert_eq!(op, crate::UnOp::Not);
        assert!(matches!(
            ast.expr(operand).kind,
            crate::ExprKind::Identifier(_)
        ));
        assert_eq!(outer.span, crate::Span::new(F, 0, 2));
    }

    #[test]
    fn parse_unary_neg() {
        let (ast, _) = parse_str("-x;");
        let (op, _) = unary(expr_of_single_stmt(&ast));
        assert_eq!(op, crate::UnOp::Neg);
    }

    #[test]
    fn parse_unary_pos() {
        let (ast, _) = parse_str("+x;");
        let (op, _) = unary(expr_of_single_stmt(&ast));
        assert_eq!(op, crate::UnOp::Pos);
    }

    #[test]
    fn repeated_unary_not() {
        let (ast, _) = parse_str("!!x;");
        let outer = expr_of_single_stmt(&ast);
        let (op, operand) = unary(outer);
        assert_eq!(op, crate::UnOp::Not);
        let (inner_op, inner_operand) = unary(ast.expr(operand));
        assert_eq!(inner_op, crate::UnOp::Not);
        assert!(matches!(
            ast.expr(inner_operand).kind,
            crate::ExprKind::Identifier(_)
        ));
    }

    #[test]
    fn unary_binds_tighter_than_binary() {
        let (ast, _) = parse_str("-x + 1;");
        let outer = expr_of_single_stmt(&ast);
        let (op, lhs, rhs) = binary(outer);
        assert_eq!(op, crate::BinOp::Add);
        let (unop, _) = unary(ast.expr(lhs));
        assert_eq!(unop, crate::UnOp::Neg);
        assert_eq!(ast.expr(rhs).kind, crate::ExprKind::Number(1.0));
    }

    #[test]
    fn unary_over_paren() {
        let (ast, _) = parse_str("-(x + 1);");
        let outer = expr_of_single_stmt(&ast);
        let (op, operand) = unary(outer);
        assert_eq!(op, crate::UnOp::Neg);
        let crate::ExprKind::Paren(inner_id) = ast.expr(operand).kind else {
            panic!("expected Paren");
        };
        let (inner_op, ..) = binary(ast.expr(inner_id));
        assert_eq!(inner_op, crate::BinOp::Add);
    }

    #[test]
    fn parse_is_as_expression_rejected() {
        let (_, diags) = parse_str("x is number;");
        assert!(!diags.is_empty(), "expected parse error for `x is T`");
    }

    fn as_cast(e: &crate::Expr) -> (crate::ExprId, &crate::TypeAnnotation) {
        match &e.kind {
            crate::ExprKind::As { expr, ty } => (*expr, ty),
            other => panic!("expected ExprKind::As, got {other:?}"),
        }
    }

    #[test]
    fn parse_instanceof_class() {
        let (ast, diags) = parse_str("x instanceof Foo;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let outer = expr_of_single_stmt(&ast);
        let crate::ExprKind::InstanceOf { value, ty } = &outer.kind else {
            panic!("expected ExprKind::InstanceOf, got {:?}", outer.kind);
        };
        assert!(matches!(
            ast.expr(*value).kind,
            crate::ExprKind::Identifier(_)
        ));
        assert!(matches!(ty.kind, crate::TypeAnnotationKind::Name { .. }));
    }

    #[test]
    fn parse_instanceof_binds_tighter_than_logical_and() {
        // `a instanceof B && c` parses as `(a instanceof B) && c`.
        let (ast, diags) = parse_str("a instanceof B && c;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let outer = expr_of_single_stmt(&ast);
        let (op, lhs, rhs) = binary(outer);
        assert_eq!(op, crate::BinOp::And);
        assert!(matches!(
            ast.expr(lhs).kind,
            crate::ExprKind::InstanceOf { .. }
        ));
        assert!(matches!(ast.expr(rhs).kind, crate::ExprKind::Identifier(_)));
    }

    #[test]
    fn parse_as_primitive() {
        let (ast, diags) = parse_str("x as number;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let outer = expr_of_single_stmt(&ast);
        let (inner, ty) = as_cast(outer);
        assert!(matches!(
            ast.expr(inner).kind,
            crate::ExprKind::Identifier(_)
        ));
        match &ty.kind {
            crate::TypeAnnotationKind::Name { args, .. } => {
                assert!(args.is_empty());
            }
            other => panic!("expected Name target, got {other:?}"),
        }
        assert_eq!(outer.span, crate::Span::new(F, 0, 11));
    }

    #[test]
    fn parse_as_object_literal_type() {
        let (ast, diags) = parse_str("x as { a: number };");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let outer = expr_of_single_stmt(&ast);
        let (_, ty) = as_cast(outer);
        assert!(matches!(ty.kind, crate::TypeAnnotationKind::Object { .. }));
    }

    #[test]
    fn parse_as_array_short() {
        let (ast, diags) = parse_str("x as string[];");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let outer = expr_of_single_stmt(&ast);
        let (_, ty) = as_cast(outer);
        assert!(matches!(ty.kind, crate::TypeAnnotationKind::Array { .. }));
    }

    #[test]
    fn parse_as_array_generic() {
        let (ast, diags) = parse_str("x as T[];");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let outer = expr_of_single_stmt(&ast);
        let (_, ty) = as_cast(outer);
        assert!(matches!(ty.kind, crate::TypeAnnotationKind::Array { .. }));
    }

    #[test]
    fn parse_as_generic_instantiation() {
        let (ast, diags) = parse_str("x as Foo<Bar>;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let outer = expr_of_single_stmt(&ast);
        let (_, ty) = as_cast(outer);
        match &ty.kind {
            crate::TypeAnnotationKind::Name { args, .. } => {
                assert_eq!(args.len(), 1, "Foo<Bar> has one type arg");
            }
            other => panic!("expected Name with type args, got {other:?}"),
        }
    }

    #[test]
    fn parse_as_precedence_higher_than_additive() {
        let (ast, diags) = parse_str("x as number + 1;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let outer = expr_of_single_stmt(&ast);
        let (op, lhs, rhs) = binary(outer);
        assert_eq!(op, crate::BinOp::Add);
        assert!(matches!(ast.expr(lhs).kind, crate::ExprKind::As { .. }));
        assert!(matches!(ast.expr(rhs).kind, crate::ExprKind::Number(_)));
    }

    #[test]
    fn parse_as_precedence_lower_than_unary() {
        let (ast, diags) = parse_str("!x as boolean;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let outer = expr_of_single_stmt(&ast);
        let (inner, _) = as_cast(outer);
        assert!(matches!(
            ast.expr(inner).kind,
            crate::ExprKind::Unary {
                op: crate::UnOp::Not,
                ..
            }
        ));
    }

    #[test]
    fn parse_as_left_associative_chain() {
        let (ast, diags) = parse_str("x as A as B;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let outer = expr_of_single_stmt(&ast);
        let (inner, _outer_ty) = as_cast(outer);
        let (innermost, _inner_ty) = as_cast(ast.expr(inner));
        assert!(matches!(
            ast.expr(innermost).kind,
            crate::ExprKind::Identifier(_)
        ));
    }

    #[test]
    fn parse_as_inside_call_arg() {
        let (ast, diags) = parse_str("f(x as T);");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let outer = expr_of_single_stmt(&ast);
        match &outer.kind {
            crate::ExprKind::Call { args, .. } => {
                assert_eq!(args.len(), 1);
                assert!(matches!(ast.expr(args[0]).kind, crate::ExprKind::As { .. }));
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn parse_as_after_field_access() {
        let (ast, diags) = parse_str("obj.field as T;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let outer = expr_of_single_stmt(&ast);
        let (inner, _) = as_cast(outer);
        assert!(matches!(
            ast.expr(inner).kind,
            crate::ExprKind::FieldAccess { .. }
        ));
    }

    #[test]
    fn parse_type_guard_return_annotation_on_function() {
        let src = "function isCircle(s: Shape): s is Circle { return true; }";
        let (ast, diags) = parse_str(src);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmt = single_stmt(&ast);
        match stmt.kind {
            crate::StmtKind::Function {
                ref return_type,
                ref type_predicate,
                ..
            } => {
                assert!(
                    return_type.is_none(),
                    "predicate form leaves return_type empty",
                );
                let tp = type_predicate.as_ref().expect("predicate parsed");
                assert_eq!(tp.param.name, "s");
                assert!(matches!(
                    tp.asserted.kind,
                    crate::TypeAnnotationKind::Name { .. }
                ));
                assert_eq!(tp.span, crate::Span::new(F, 29, 40));
            }
            _ => panic!("expected Function"),
        }
    }

    #[test]
    fn parse_type_guard_return_annotation_on_arrow() {
        let src = "const f = (s: Shape): s is Circle => true;";
        let (ast, diags) = parse_str(src);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let value = match &ast.stmt(ast.top_level[0]).kind {
            crate::StmtKind::Const { value, .. } => *value,
            _ => panic!("expected const"),
        };
        match &ast.expr(value).kind {
            crate::ExprKind::Arrow {
                return_type,
                type_predicate,
                ..
            } => {
                assert!(return_type.is_none());
                let tp = type_predicate.as_ref().expect("predicate parsed");
                assert_eq!(tp.param.name, "s");
            }
            other => panic!("expected Arrow, got {other:?}"),
        }
    }

    #[test]
    fn parse_plain_return_type_unaffected_by_predicate_parser() {
        let src = "function f(s: Shape): boolean { return true; }";
        let (ast, diags) = parse_str(src);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmt = single_stmt(&ast);
        match stmt.kind {
            crate::StmtKind::Function {
                ref return_type,
                ref type_predicate,
                ..
            } => {
                assert!(return_type.is_some(), "plain form has return_type");
                assert!(type_predicate.is_none(), "plain form has no type_predicate",);
            }
            _ => panic!("expected Function"),
        }
    }

    #[test]
    fn parse_type_guard_rejects_non_return_position() {
        let (_, diags) = parse_str("let x: y is Foo = null;");
        assert!(
            !diags.is_empty(),
            "expected parse error for `is` outside return-type position",
        );
    }

    #[test]
    fn parse_typeof_prefix() {
        let (ast, diags) = parse_str("typeof x;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let outer = expr_of_single_stmt(&ast);
        let crate::ExprKind::Typeof { operand } = outer.kind else {
            panic!("expected Typeof, got {:?}", outer.kind);
        };
        assert!(matches!(
            ast.expr(operand).kind,
            crate::ExprKind::Identifier(_)
        ));
        assert_eq!(outer.span, crate::Span::new(F, 0, 8));
    }

    #[test]
    fn parse_typeof_in_equality() {
        let (ast, diags) = parse_str(r#"typeof x === "number";"#);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let outer = expr_of_single_stmt(&ast);
        let (op, lhs, rhs) = binary(outer);
        assert_eq!(op, crate::BinOp::Eq);
        assert!(matches!(ast.expr(lhs).kind, crate::ExprKind::Typeof { .. }));
        assert!(matches!(
            ast.expr(rhs).kind,
            crate::ExprKind::String(ref s) if s == "number"
        ));
    }

    #[test]
    fn parse_let_without_type() {
        let (ast, diags) = parse_str("let x = 1;");
        assert!(diags.is_empty());
        let stmt = single_stmt(&ast);
        assert_eq!(stmt.span, crate::Span::new(F, 0, 10));
        match stmt.kind {
            crate::StmtKind::Let {
                ref name,
                ref ty,
                value,
                ..
            } => {
                assert_eq!(name.name, "x");
                assert_eq!(name.span, crate::Span::new(F, 4, 5));
                assert!(ty.is_none());
                assert_eq!(ast.expr(value).kind, crate::ExprKind::Number(1.0));
            }
            _ => panic!("expected Let"),
        }
    }

    #[test]
    fn parse_const_with_type_and_string() {
        let (ast, diags) = parse_str(r#"const y: string = "hi";"#);
        assert!(diags.is_empty());
        let stmt = single_stmt(&ast);
        match stmt.kind {
            crate::StmtKind::Const {
                ref name,
                ref ty,
                value,
                ..
            } => {
                assert_eq!(name.name, "y");
                assert_eq!(name.span, crate::Span::new(F, 6, 7));
                let ty = ty.as_ref().expect("expected type annotation");
                assert!(matches!(ty.kind, crate::TypeAnnotationKind::Name { .. }));
                assert_eq!(ty.span, crate::Span::new(F, 9, 15));
                assert_eq!(
                    ast.expr(value).kind,
                    crate::ExprKind::String("hi".to_string())
                );
            }
            _ => panic!("expected Const"),
        }
    }

    #[test]
    fn parse_let_with_type_and_binary_initializer() {
        let (ast, diags) = parse_str("let z: number = 1 + 2;");
        assert!(diags.is_empty());
        let stmt = single_stmt(&ast);
        match stmt.kind {
            crate::StmtKind::Let { ref ty, value, .. } => {
                assert!(ty.is_some());
                let (op, ..) = binary(ast.expr(value));
                assert_eq!(op, crate::BinOp::Add);
            }
            _ => panic!("expected Let"),
        }
    }

    #[test]
    fn void_type_annotation_is_accepted_syntactically() {
        let (ast, diags) = parse_str("let v: void = null;");
        assert!(diags.is_empty());
        let stmt = single_stmt(&ast);
        match stmt.kind {
            crate::StmtKind::Let { ref ty, .. } => {
                let ty = ty.as_ref().unwrap();
                assert!(matches!(ty.kind, crate::TypeAnnotationKind::Name { .. }));
                assert_eq!(ty.span, crate::Span::new(F, 7, 11));
            }
            _ => panic!("expected Let"),
        }
    }

    #[test]
    fn const_missing_initializer_diagnoses() {
        let (ast, diags) = parse_str("const x;");
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].message,
            "`const` declaration requires an initializer"
        );
        assert!(ast.top_level.is_empty());
    }

    #[test]
    fn parse_const_object_shorthand_pattern() {
        let (ast, diags) = parse_str("const { a, b } = obj;");
        assert!(diags.is_empty(), "unexpected: {diags:?}");
        let stmt = single_stmt(&ast);
        match stmt.kind {
            crate::StmtKind::ConstPattern {
                binding:
                    crate::Binding::Object {
                        ref fields,
                        ref rest,
                        ..
                    },
                ..
            } => {
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].source.name, "a");
                assert_eq!(fields[0].local.name, "a");
                assert_eq!(fields[1].source.name, "b");
                assert_eq!(fields[1].local.name, "b");
                assert!(rest.is_none());
            }
            _ => panic!("expected ConstPattern Object, got {:?}", stmt.kind),
        }
    }

    #[test]
    fn parse_const_object_pattern_keyword_source_key() {
        let (ast, diags) = parse_str("const { type: statusType, default: fallback } = obj;");
        assert!(diags.is_empty(), "unexpected: {diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::ConstPattern {
            binding: crate::Binding::Object { ref fields, .. },
            ..
        } = stmt.kind
        else {
            panic!("expected object const pattern");
        };

        let names: Vec<(&str, &str)> = fields
            .iter()
            .map(|f| (f.source.name.as_str(), f.local.name.as_str()))
            .collect();
        assert_eq!(names, vec![("type", "statusType"), ("default", "fallback")]);
    }

    #[test]
    fn parse_const_object_pattern_keyword_shorthand_rejected() {
        let (_ast, diags) = parse_str("const { default } = obj;");
        assert!(
            diags
                .iter()
                .any(|d| d.message == "expected `:` after keyword field name in object pattern"),
            "expected keyword-shorthand diagnostic, got: {diags:?}"
        );
    }

    #[test]
    fn parse_const_object_renamed_pattern() {
        let (ast, diags) = parse_str("const { a: x, b: y } = obj;");
        assert!(diags.is_empty(), "unexpected: {diags:?}");
        match single_stmt(&ast).kind {
            crate::StmtKind::ConstPattern {
                binding: crate::Binding::Object { ref fields, .. },
                ..
            } => {
                assert_eq!(fields[0].source.name, "a");
                assert_eq!(fields[0].local.name, "x");
                assert_eq!(fields[1].source.name, "b");
                assert_eq!(fields[1].local.name, "y");
            }
            ref other => panic!("expected ConstPattern Object, got {other:?}"),
        }
    }

    #[test]
    fn parse_const_object_with_rest() {
        let (ast, diags) = parse_str("const { a, ...rest } = obj;");
        assert!(diags.is_empty(), "unexpected: {diags:?}");
        match single_stmt(&ast).kind {
            crate::StmtKind::ConstPattern {
                binding:
                    crate::Binding::Object {
                        ref fields,
                        ref rest,
                        ..
                    },
                ..
            } => {
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].source.name, "a");
                let r = rest.as_ref().expect("expected rest");
                assert_eq!(r.name, "rest");
            }
            ref other => panic!("expected ConstPattern Object, got {other:?}"),
        }
    }

    #[test]
    fn parse_const_array_pattern() {
        let (ast, diags) = parse_str("const [x, y] = arr;");
        assert!(diags.is_empty(), "unexpected: {diags:?}");
        match single_stmt(&ast).kind {
            crate::StmtKind::ConstPattern {
                binding:
                    crate::Binding::Array {
                        ref elems,
                        ref rest,
                        ..
                    },
                ..
            } => {
                assert_eq!(elems.len(), 2);
                assert_eq!(elems[0].as_ref().unwrap().name, "x");
                assert_eq!(elems[1].as_ref().unwrap().name, "y");
                assert!(rest.is_none());
            }
            ref other => panic!("expected ConstPattern Array, got {other:?}"),
        }
    }

    #[test]
    fn parse_const_array_with_hole_and_rest() {
        let (ast, diags) = parse_str("const [, x, ...rest] = arr;");
        assert!(diags.is_empty(), "unexpected: {diags:?}");
        match single_stmt(&ast).kind {
            crate::StmtKind::ConstPattern {
                binding:
                    crate::Binding::Array {
                        ref elems,
                        ref rest,
                        ..
                    },
                ..
            } => {
                assert_eq!(elems.len(), 2);
                assert!(elems[0].is_none(), "first slot should be a hole");
                assert_eq!(elems[1].as_ref().unwrap().name, "x");
                assert_eq!(rest.as_ref().unwrap().name, "rest");
            }
            ref other => panic!("expected ConstPattern Array, got {other:?}"),
        }
    }

    #[test]
    fn parse_let_object_pattern() {
        let (ast, diags) = parse_str("let { a, b } = obj;");
        assert!(diags.is_empty(), "unexpected: {diags:?}");
        assert!(matches!(
            single_stmt(&ast).kind,
            crate::StmtKind::LetPattern {
                binding: crate::Binding::Object { .. },
                ..
            }
        ));
    }

    #[test]
    fn parse_function_param_object_pattern() {
        let src = "function f({ a, b }: T): void { return; }";
        let (ast, diags) = parse_str(src);
        assert!(diags.is_empty(), "unexpected: {diags:?}");
        match single_stmt(&ast).kind {
            crate::StmtKind::Function { ref params, .. } => {
                assert_eq!(params.len(), 1);
                let p = &params[0];
                assert!(p.pattern.is_some(), "expected pattern param");
                assert_eq!(p.name.name, "", "pattern placeholder uses empty name");
                match p.pattern.as_ref().unwrap() {
                    crate::Binding::Object { fields, .. } => {
                        assert_eq!(fields.len(), 2);
                        assert_eq!(fields[0].source.name, "a");
                    }
                    other => panic!("expected Object binding, got {other:?}"),
                }
            }
            ref other => panic!("expected Function, got {other:?}"),
        }
    }

    #[test]
    fn parse_arrow_param_array_pattern() {
        let src = "const g = ([x, y]: number[]): number => x + y;";
        let (_ast, diags) = parse_str(src);
        assert!(diags.is_empty(), "unexpected: {diags:?}");
    }

    #[test]
    fn parse_destructure_nested_rejected() {
        let (_ast, diags) = parse_str("const { a: { b } } = obj;");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("nested destructuring")),
            "expected nested-destructuring diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn parse_destructure_array_nested_rejected() {
        let (_ast, diags) = parse_str("const [[a]] = arr;");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("nested destructuring")),
            "expected nested-destructuring diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn parse_destructure_pattern_default_rejected() {
        let (_ast, diags) = parse_str("const { a = 1 } = obj;");
        assert!(
            diags.iter().any(|d| d
                .message
                .contains("default values inside destructuring patterns")),
            "expected pattern-default diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn parse_destructure_param_default_rejected() {
        let src = "function f({ a }: T = obj): void { return; }";
        let (_ast, diags) = parse_str(src);
        assert!(
            diags.iter().any(|d| d
                .message
                .contains("default values are not supported on destructured parameters")),
            "expected param-default-on-pattern diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn parse_destructure_rest_not_last_rejected() {
        let (_ast, diags) = parse_str("const { ...rest, a } = obj;");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("rest element must be the last element")),
            "expected rest-not-last diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn parse_destructure_empty_object_rejected() {
        let (_ast, diags) = parse_str("const { } = obj;");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("empty object destructuring pattern")),
            "expected empty-pattern diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn parse_ternary_simple() {
        let (ast, diags) = parse_str("const x: number = a ? 1 : 2;");
        assert!(diags.is_empty(), "unexpected: {diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::Const { value, .. } = stmt.kind else {
            panic!("expected Const, got {:?}", stmt.kind);
        };
        assert!(matches!(
            ast.expr(value).kind,
            crate::ExprKind::Ternary { .. }
        ));
    }

    #[test]
    fn parse_ternary_nested_right_assoc() {
        let (ast, diags) = parse_str("const x: number = a ? 1 : b ? 2 : 3;");
        assert!(diags.is_empty(), "unexpected: {diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::Const { value, .. } = stmt.kind else {
            panic!("expected Const");
        };
        let crate::ExprKind::Ternary { else_, .. } = ast.expr(value).kind else {
            panic!("expected outer Ternary");
        };
        assert!(matches!(
            ast.expr(else_).kind,
            crate::ExprKind::Ternary { .. }
        ));
    }

    #[test]
    fn parse_ternary_missing_colon_diagnoses() {
        let (_, diags) = parse_str("const x: number = a ? 1;");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("expected `:` to complete ternary")),
            "expected colon diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn parse_nullish_coalesce_binop() {
        let (ast, diags) = parse_str("const x: number = a ?? 1;");
        assert!(diags.is_empty(), "unexpected: {diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::Const { value, .. } = stmt.kind else {
            panic!("expected Const");
        };
        match ast.expr(value).kind {
            crate::ExprKind::Binary { op, .. } => {
                assert_eq!(op, crate::BinOp::NullishCoalesce);
            }
            ref other => panic!("expected Binary NullishCoalesce, got {other:?}"),
        }
    }

    #[test]
    fn parse_nullish_left_associative() {
        let (ast, diags) = parse_str("const x: number = a ?? b ?? c;");
        assert!(diags.is_empty(), "unexpected: {diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::Const { value, .. } = stmt.kind else {
            panic!("expected Const");
        };
        let crate::ExprKind::Binary { lhs, .. } = ast.expr(value).kind else {
            panic!("expected outer Binary");
        };
        match ast.expr(lhs).kind {
            crate::ExprKind::Binary { op, .. } => {
                assert_eq!(op, crate::BinOp::NullishCoalesce);
            }
            ref other => panic!("expected inner Binary, got {other:?}"),
        }
    }

    #[test]
    fn parse_pow_is_right_associative() {
        let (ast, diags) = parse_str("const x: number = 2 ** 3 ** 2;");
        assert!(diags.is_empty(), "unexpected: {diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::Const { value, .. } = stmt.kind else {
            panic!("expected Const");
        };
        let crate::ExprKind::Binary { op, rhs, .. } = ast.expr(value).kind else {
            panic!("expected outer Binary");
        };
        assert_eq!(op, crate::BinOp::Pow);
        match ast.expr(rhs).kind {
            crate::ExprKind::Binary { op: inner_op, .. } => {
                assert_eq!(inner_op, crate::BinOp::Pow);
            }
            ref other => panic!("expected inner Pow Binary on RHS, got {other:?}"),
        }
    }

    #[test]
    fn parse_pow_higher_than_multiply() {
        let (ast, diags) = parse_str("const x: number = 2 * 3 ** 2;");
        assert!(diags.is_empty(), "unexpected: {diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::Const { value, .. } = stmt.kind else {
            panic!("expected Const");
        };
        let crate::ExprKind::Binary { op, rhs, .. } = ast.expr(value).kind else {
            panic!("expected outer Binary");
        };
        assert_eq!(op, crate::BinOp::Mul);
        match ast.expr(rhs).kind {
            crate::ExprKind::Binary { op: inner_op, .. } => {
                assert_eq!(inner_op, crate::BinOp::Pow);
            }
            ref other => panic!("expected Pow on the RHS of *, got {other:?}"),
        }
    }

    #[test]
    fn parse_compound_assign_ident() {
        let (ast, diags) = parse_str("function main(): void { let x: number = 0; x += 5; }");
        assert!(diags.is_empty(), "unexpected: {diags:?}");
        let func = ast.top_level.iter().find_map(|sid| {
            if let crate::StmtKind::Function { body, .. } = &ast.stmt(*sid).kind {
                Some(*body)
            } else {
                None
            }
        });
        let crate::StmtKind::Block(stmts) = &ast.stmt(func.expect("main")).kind else {
            panic!("expected function body to be a block");
        };
        let target = stmts.iter().find_map(|sid| match &ast.stmt(*sid).kind {
            crate::StmtKind::CompoundAssign { target, op, .. } => Some((target.name.clone(), *op)),
            _ => None,
        });
        let (name, op) = target.expect("expected a CompoundAssign in the body");
        assert_eq!(name, "x");
        assert_eq!(op, crate::BinOp::Add);
    }

    #[test]
    fn parse_nullish_mixed_with_or_rejected() {
        let (_, diags) = parse_str("const x: number = a || b ?? c;");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("mixing `??` with `||` / `&&`")),
            "expected mixing diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn parse_nullish_mixed_with_and_rejected() {
        let (_, diags) = parse_str("const x: number = a && b ?? c;");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("mixing `??` with `||` / `&&`")),
            "expected mixing diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn parse_nullish_with_parens_ok() {
        let (_, diags) = parse_str("const x: number = (a || b) ?? c;");
        assert!(diags.is_empty(), "unexpected: {diags:?}");
    }

    #[test]
    fn parse_optional_field_chain() {
        let (ast, diags) = parse_str("const x: number = a?.b;");
        assert!(diags.is_empty(), "unexpected: {diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::Const { value, .. } = stmt.kind else {
            panic!("expected Const");
        };
        match ast.expr(value).kind {
            crate::ExprKind::OptionalChain { ref parts, .. } => {
                assert_eq!(parts.len(), 1);
                match &parts[0] {
                    crate::ChainPart::Field { name, optional, .. } => {
                        assert_eq!(name.name, "b");
                        assert!(*optional);
                    }
                    other => panic!("expected Field, got {other:?}"),
                }
            }
            ref other => panic!("expected OptionalChain, got {other:?}"),
        }
    }

    #[test]
    fn parse_optional_chain_continues_after_first_dot() {
        let (ast, diags) = parse_str("const x: number = a?.b.c;");
        assert!(diags.is_empty(), "unexpected: {diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::Const { value, .. } = stmt.kind else {
            panic!("expected Const");
        };
        match ast.expr(value).kind {
            crate::ExprKind::OptionalChain { ref parts, .. } => {
                assert_eq!(parts.len(), 2);
                if let crate::ChainPart::Field { optional, .. } = &parts[0] {
                    assert!(*optional);
                }
                if let crate::ChainPart::Field { optional, .. } = &parts[1] {
                    assert!(!*optional);
                }
            }
            ref other => panic!("expected OptionalChain, got {other:?}"),
        }
    }

    #[test]
    fn parse_optional_chain_multiple_short_circuits() {
        let (ast, diags) = parse_str("const x: number = a?.b?.c;");
        assert!(diags.is_empty(), "unexpected: {diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::Const { value, .. } = stmt.kind else {
            panic!("expected Const");
        };
        let crate::ExprKind::OptionalChain { parts, .. } = &ast.expr(value).kind else {
            panic!("expected OptionalChain");
        };
        assert_eq!(parts.len(), 2);
        for p in parts {
            if let crate::ChainPart::Field { optional, .. } = p {
                assert!(*optional, "both parts should be optional");
            }
        }
    }

    #[test]
    fn parse_optional_call_and_index() {
        let (_, diags) = parse_str("const x: number = f?.();");
        assert!(diags.is_empty(), "unexpected: {diags:?}");
        let (_, diags) = parse_str("const y: number = arr?.[0];");
        assert!(diags.is_empty(), "unexpected: {diags:?}");
    }

    #[test]
    fn parse_optional_chain_terminates_at_questionmark_for_ternary() {
        let (ast, diags) = parse_str("const x: number = a?.b ? c : d;");
        assert!(diags.is_empty(), "unexpected: {diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::Const { value, .. } = stmt.kind else {
            panic!("expected Const");
        };
        let crate::ExprKind::Ternary { cond, .. } = ast.expr(value).kind else {
            panic!("expected top-level Ternary, got {:?}", ast.expr(value).kind);
        };
        assert!(matches!(
            ast.expr(cond).kind,
            crate::ExprKind::OptionalChain { .. }
        ));
    }

    #[test]
    fn parse_nullish_below_ternary() {
        let (ast, diags) = parse_str("const x: number = a ?? b ? c : d;");
        assert!(diags.is_empty(), "unexpected: {diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::Const { value, .. } = stmt.kind else {
            panic!("expected Const");
        };
        let crate::ExprKind::Ternary { cond, .. } = ast.expr(value).kind else {
            panic!("expected top-level Ternary");
        };
        match ast.expr(cond).kind {
            crate::ExprKind::Binary { op, .. } => {
                assert_eq!(op, crate::BinOp::NullishCoalesce);
            }
            ref other => panic!("expected Binary ??, got {other:?}"),
        }
    }

    #[test]
    fn let_missing_initializer_diagnoses() {
        let (ast, diags) = parse_str("let x;");
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].message,
            "`let` declaration requires an initializer"
        );
        assert!(ast.top_level.is_empty());
    }

    #[test]
    fn missing_identifier_after_let_diagnoses() {
        let (ast, diags) = parse_str("let = 1;");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "expected identifier after `let`/`const`");
        assert!(ast.top_level.is_empty());
    }

    #[test]
    fn missing_type_after_colon_diagnoses() {
        let (ast, diags) = parse_str("let x: = 1;");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "expected type");
        assert!(ast.top_level.is_empty());
    }

    #[test]
    fn missing_semicolon_after_declaration_diagnoses() {
        let (ast, diags) = parse_str("let x = 1 y;");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "expected `;` after declaration");
        assert!(ast.top_level.is_empty());
    }

    #[test]
    fn expression_statements_still_parse() {
        let (ast, diags) = parse_str("42;");
        assert!(diags.is_empty());
        assert_eq!(ast.top_level.len(), 1);
    }

    #[test]
    fn snapshot_mixed_declarations() {
        let (ast, diags) =
            parse_str(r#"let x = 1; const y: string = "hi"; let z: number = 1 + 2;"#);
        assert!(diags.is_empty());
        assert_eq!(ast.top_level.len(), 3);
        insta::assert_debug_snapshot!(ast);
    }

    #[test]
    fn parse_function_typical() {
        let (ast, diags) = parse_str("function f(a: number, b: string): boolean { }");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmt = single_stmt(&ast);
        match stmt.kind {
            crate::StmtKind::Function {
                ref name,
                ref generics,
                ref params,
                ref return_type,
                body,
                ..
            } => {
                assert_eq!(name.name, "f");
                assert_eq!(name.span, crate::Span::new(F, 9, 10));
                assert!(generics.is_empty());
                assert_eq!(params.len(), 2);
                assert_eq!(params[0].name.name, "a");
                assert_eq!(params[0].name.span, crate::Span::new(F, 11, 12));
                let p0_ty = params[0]
                    .ty
                    .as_ref()
                    .expect("function-decl param requires annotation");
                assert!(matches!(p0_ty.kind, crate::TypeAnnotationKind::Name { .. }));
                assert_eq!(p0_ty.span, crate::Span::new(F, 14, 20)); // `number`
                assert_eq!(params[1].name.name, "b");
                assert_eq!(params[1].name.span, crate::Span::new(F, 22, 23));
                let p1_ty = params[1]
                    .ty
                    .as_ref()
                    .expect("function-decl param requires annotation");
                assert_eq!(p1_ty.span, crate::Span::new(F, 25, 31)); // `string`
                let return_type = return_type.as_ref().expect("plain return type");
                assert!(matches!(
                    return_type.kind,
                    crate::TypeAnnotationKind::Name { .. }
                ));
                assert_eq!(return_type.span, crate::Span::new(F, 34, 41)); // `boolean`
                let block = ast.stmt(body);
                assert!(matches!(block.kind, crate::StmtKind::Block(ref v) if v.is_empty()));
            }
            _ => panic!("expected Function"),
        }
    }

    #[test]
    fn parse_function_with_doc_comment() {
        let (ast, diags) = parse_str(
            "/**\n * Sum two numbers.\n * @param a First.\n * @param b Second.\n */\nfunction add(a: number, b: number): number { return a + b; }",
        );
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmt = single_stmt(&ast);
        match &stmt.kind {
            crate::StmtKind::Function { doc, .. } => {
                let d = doc.as_ref().expect("doc attached");
                assert_eq!(d.summary, "Sum two numbers.");
                assert_eq!(d.params.len(), 2);
                assert_eq!(d.params[0].name, "a");
                assert_eq!(d.params[1].name, "b");
            }
            _ => panic!("expected Function"),
        }
    }

    #[test]
    fn parse_interface_member_docs() {
        let (ast, diags) = parse_str(
            "interface Foo {\n  /** First method. */ first(): void;\n  /** A field. */ field: number;\n}",
        );
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmt = single_stmt(&ast);
        match &stmt.kind {
            crate::StmtKind::InterfaceDecl { members, .. } => {
                assert_eq!(members.len(), 2);
                match &members[0] {
                    crate::InterfaceMember::Method { doc, .. } => {
                        assert_eq!(doc.as_ref().unwrap().summary, "First method.");
                    }
                    _ => panic!("expected Method first"),
                }
                match &members[1] {
                    crate::InterfaceMember::Property { doc, .. } => {
                        assert_eq!(doc.as_ref().unwrap().summary, "A field.");
                    }
                    _ => panic!("expected Property second"),
                }
            }
            _ => panic!("expected InterfaceDecl"),
        }
    }

    #[test]
    fn parse_numeric_enum_all_implicit() {
        let (ast, diags) = parse_str("enum Direction { Up, Down, Left, Right }");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmt = single_stmt(&ast);
        match &stmt.kind {
            crate::StmtKind::EnumDecl { name, members, doc } => {
                assert_eq!(name.name, "Direction");
                assert!(doc.is_none());
                assert_eq!(members.len(), 4);
                for (m, expected) in members.iter().zip(["Up", "Down", "Left", "Right"]) {
                    assert_eq!(m.name.name, expected);
                    assert!(m.value.is_none(), "expected no initializer on {expected}");
                }
            }
            _ => panic!("expected EnumDecl"),
        }
    }

    #[test]
    fn parse_string_enum_all_explicit() {
        let (ast, diags) =
            parse_str("enum Status { Active = \"active\", Inactive = \"inactive\" }");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmt = single_stmt(&ast);
        match &stmt.kind {
            crate::StmtKind::EnumDecl { name, members, .. } => {
                assert_eq!(name.name, "Status");
                assert_eq!(members.len(), 2);
                match &members[0].value {
                    Some(crate::EnumInitializer::String { value, .. }) => {
                        assert_eq!(value, "active");
                    }
                    other => panic!("expected String init, got {other:?}"),
                }
                match &members[1].value {
                    Some(crate::EnumInitializer::String { value, .. }) => {
                        assert_eq!(value, "inactive");
                    }
                    other => panic!("expected String init, got {other:?}"),
                }
            }
            _ => panic!("expected EnumDecl"),
        }
    }

    #[test]
    fn parse_numeric_enum_with_negative_init() {
        let (ast, diags) = parse_str("enum D { Up = 1, Down = -1 }");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmt = single_stmt(&ast);
        match &stmt.kind {
            crate::StmtKind::EnumDecl { members, .. } => {
                assert_eq!(members.len(), 2);
                match &members[0].value {
                    Some(crate::EnumInitializer::Number { value, .. }) => {
                        assert_eq!(*value, 1.0);
                    }
                    other => panic!("expected Number init, got {other:?}"),
                }
                match &members[1].value {
                    Some(crate::EnumInitializer::Number { value, .. }) => {
                        assert_eq!(*value, -1.0);
                    }
                    other => panic!("expected Number init, got {other:?}"),
                }
            }
            _ => panic!("expected EnumDecl"),
        }
    }

    #[test]
    fn parse_enum_mixed_kinds_is_parse_error() {
        let (_ast, diags) = parse_str("enum Mix { A = 1, B = \"two\" }");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("mixed numeric and string")),
            "expected mixed-kind diagnostic, got: {diags:?}"
        );
    }

    #[test]
    fn parse_enum_member_docs() {
        let (ast, diags) = parse_str(
            "enum D {\n  /** The first one. */ A = 1,\n  /** The second one. */ B = 2,\n}",
        );
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmt = single_stmt(&ast);
        match &stmt.kind {
            crate::StmtKind::EnumDecl { members, .. } => {
                assert_eq!(members.len(), 2);
                assert_eq!(
                    members[0].doc.as_ref().expect("first doc").summary,
                    "The first one."
                );
                assert_eq!(
                    members[1].doc.as_ref().expect("second doc").summary,
                    "The second one."
                );
            }
            _ => panic!("expected EnumDecl"),
        }
    }

    #[test]
    fn parse_enum_trailing_comma_ok() {
        let (ast, diags) = parse_str("enum D { A, B, }");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmt = single_stmt(&ast);
        match &stmt.kind {
            crate::StmtKind::EnumDecl { members, .. } => assert_eq!(members.len(), 2),
            _ => panic!("expected EnumDecl"),
        }
    }

    #[test]
    fn parse_function_empty_params() {
        let (ast, diags) = parse_str("function noop(): void { }");
        assert!(diags.is_empty());
        let stmt = single_stmt(&ast);
        match stmt.kind {
            crate::StmtKind::Function { ref params, .. } => assert!(params.is_empty()),
            _ => panic!("expected Function"),
        }
    }

    #[test]
    fn parse_generic_function_single_param() {
        let (ast, diags) = parse_str("function identity<T>(x: T): T { return x; }");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmt = single_stmt(&ast);
        match stmt.kind {
            crate::StmtKind::Function {
                ref name,
                ref generics,
                ref params,
                ..
            } => {
                assert_eq!(name.name, "identity");
                assert_eq!(generics.len(), 1);
                assert_eq!(generics[0].name, "T");
                assert_eq!(params.len(), 1);
                assert_eq!(params[0].name.name, "x");
            }
            _ => panic!("expected Function"),
        }
    }

    #[test]
    fn parse_generic_function_multiple_params() {
        let (ast, diags) = parse_str("function pair<T, U>(a: T, b: U): T { return a; }");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmt = single_stmt(&ast);
        match stmt.kind {
            crate::StmtKind::Function { ref generics, .. } => {
                assert_eq!(generics.len(), 2);
                assert_eq!(generics[0].name, "T");
                assert_eq!(generics[1].name, "U");
            }
            _ => panic!("expected Function"),
        }
    }

    #[test]
    fn parse_non_generic_function_has_empty_generics() {
        let (ast, diags) = parse_str("function noop(): void { }");
        assert!(diags.is_empty());
        let stmt = single_stmt(&ast);
        match stmt.kind {
            crate::StmtKind::Function { ref generics, .. } => assert!(generics.is_empty()),
            _ => panic!("expected Function"),
        }
    }

    #[test]
    fn parse_empty_generic_list_diagnoses() {
        let (_ast, diags) = parse_str("function f<>(): void { }");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("empty generic parameter list")),
            "expected empty-generic-list diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn parse_trailing_comma_in_generic_list_ok() {
        let (ast, diags) = parse_str("function f<T,>(): void { }");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        match single_stmt(&ast).kind {
            crate::StmtKind::Function { ref generics, .. } => assert_eq!(generics.len(), 1),
            _ => panic!("expected Function"),
        }
    }

    #[test]
    fn parse_generic_call_single_type_arg() {
        let (ast, diags) = parse_str("identity<number>(42);");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::Expr(call_id) = stmt.kind else {
            panic!("expected Expr stmt");
        };
        match &ast.expr(call_id).kind {
            crate::ExprKind::Call {
                type_args, args, ..
            } => {
                let ta = type_args.as_ref().expect("type_args set for generic call");
                assert_eq!(ta.len(), 1);
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn parse_generic_call_trailing_comma_ok() {
        let (ast, diags) = parse_str("identity<number,>(42);");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::Expr(call_id) = stmt.kind else {
            panic!("expected Expr stmt");
        };
        match &ast.expr(call_id).kind {
            crate::ExprKind::Call {
                type_args, args, ..
            } => {
                assert_eq!(type_args.as_ref().expect("type_args set").len(), 1);
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn parse_generic_call_multi_type_args() {
        let (ast, diags) = parse_str("pair<number, string>(1, \"x\");");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::Expr(call_id) = stmt.kind else {
            panic!("expected Expr stmt");
        };
        match &ast.expr(call_id).kind {
            crate::ExprKind::Call { type_args, .. } => {
                let ta = type_args.as_ref().expect("type_args set for generic call");
                assert_eq!(ta.len(), 2);
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn parse_comparison_chain_unaffected_by_generic_lookahead() {
        let (ast, diags) = parse_str("let z = a < b > c;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert!(
            !ast_contains_call_with_type_args(&ast),
            "should not have parsed any generic call",
        );
    }

    #[test]
    fn parse_lt_with_paren_rhs_is_comparison_not_generic() {
        let (ast, diags) = parse_str("let z = a < (b);");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert!(!ast_contains_call_with_type_args(&ast));
    }

    #[test]
    fn parse_generic_call_no_args() {
        let (ast, diags) = parse_str("none<number>();");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::Expr(call_id) = stmt.kind else {
            panic!("expected Expr stmt");
        };
        match &ast.expr(call_id).kind {
            crate::ExprKind::Call {
                type_args, args, ..
            } => {
                assert_eq!(type_args.as_ref().unwrap().len(), 1);
                assert!(args.is_empty());
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    fn ast_contains_call_with_type_args(ast: &crate::Ast) -> bool {
        for id in 0..ast.top_level.len() {
            let stmt = ast.stmt(ast.top_level[id]);
            if walk_stmt_for_typed_call(ast, stmt) {
                return true;
            }
        }
        false
    }

    fn walk_stmt_for_typed_call(ast: &crate::Ast, stmt: &crate::Stmt) -> bool {
        match &stmt.kind {
            crate::StmtKind::Let { value, .. } | crate::StmtKind::Const { value, .. } => {
                walk_expr_for_typed_call(ast, *value)
            }
            crate::StmtKind::Expr(e) => walk_expr_for_typed_call(ast, *e),
            crate::StmtKind::Block(stmts) => stmts
                .iter()
                .any(|sid| walk_stmt_for_typed_call(ast, ast.stmt(*sid))),
            _ => false,
        }
    }

    fn walk_expr_for_typed_call(ast: &crate::Ast, id: crate::ExprId) -> bool {
        match &ast.expr(id).kind {
            crate::ExprKind::Call {
                type_args: Some(_), ..
            } => true,
            crate::ExprKind::Call { callee, args, .. } => {
                walk_expr_for_typed_call(ast, *callee)
                    || args.iter().any(|a| walk_expr_for_typed_call(ast, *a))
            }
            crate::ExprKind::Binary { lhs, rhs, .. } => {
                walk_expr_for_typed_call(ast, *lhs) || walk_expr_for_typed_call(ast, *rhs)
            }
            crate::ExprKind::Unary { operand, .. } => walk_expr_for_typed_call(ast, *operand),
            crate::ExprKind::Paren(e) => walk_expr_for_typed_call(ast, *e),
            crate::ExprKind::FieldAccess { receiver, .. } => {
                walk_expr_for_typed_call(ast, *receiver)
            }
            crate::ExprKind::IndexAccess { receiver, index } => {
                walk_expr_for_typed_call(ast, *receiver) || walk_expr_for_typed_call(ast, *index)
            }
            _ => false,
        }
    }

    #[test]
    fn parse_function_body_with_statements() {
        let (ast, diags) = parse_str("function g(x: number): number { let y = x; y; }");
        assert!(diags.is_empty());
        let stmt = single_stmt(&ast);
        match stmt.kind {
            crate::StmtKind::Function { body, .. } => {
                let block = ast.stmt(body);
                match block.kind {
                    crate::StmtKind::Block(ref v) => assert_eq!(v.len(), 2),
                    _ => panic!("expected Block"),
                }
            }
            _ => panic!("expected Function"),
        }
    }

    #[test]
    fn function_missing_return_type_diagnoses() {
        let (ast, diags) = parse_str("function f() { }");
        assert!(!diags.is_empty());
        assert_eq!(diags[0].message, "expected `:` and return type");
        assert!(ast.top_level.is_empty());
    }

    #[test]
    fn function_missing_param_type_diagnoses() {
        let (ast, diags) = parse_str("function f(a): void { }");
        assert!(!diags.is_empty());
        assert_eq!(diags[0].message, "parameter requires a type annotation");
        assert!(ast.top_level.is_empty());
    }

    #[test]
    fn function_missing_name_diagnoses() {
        let (ast, diags) = parse_str("function (x: number): void { }");
        assert!(!diags.is_empty());
        assert_eq!(diags[0].message, "expected function name");
        assert!(ast.top_level.is_empty());
    }

    #[test]
    fn function_trailing_param_comma_ok() {
        let (ast, diags) = parse_str("function f(a: number,): void { }");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        match single_stmt(&ast).kind {
            crate::StmtKind::Function { ref params, .. } => assert_eq!(params.len(), 1),
            _ => panic!("expected Function"),
        }
    }

    #[test]
    fn function_missing_open_brace_diagnoses() {
        let (ast, diags) = parse_str("function f(): void");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "expected `{`");
        assert!(ast.top_level.is_empty());
    }

    #[test]
    fn snapshot_function_declaration() {
        let (ast, diags) =
            parse_str("function add(a: number, b: number): number { let sum = a + b; sum; }");
        assert!(diags.is_empty());
        insta::assert_debug_snapshot!(ast);
    }

    #[test]
    fn rest_param_basic_parses() {
        let (_, diags) = parse_str("function sum(...nums: number[]): number { return 0; }");
        assert!(diags.is_empty(), "expected clean parse, got {diags:?}");
    }

    #[test]
    fn rest_param_with_fixed_prefix_parses() {
        let (_, diags) =
            parse_str("function tag(label: string, ...vals: number[]): string { return label; }");
        assert!(diags.is_empty(), "expected clean parse, got {diags:?}");
    }

    #[test]
    fn rest_param_in_arrow_parses() {
        let (_, diags) = parse_str("const f = (...n: number[]) => n.length;");
        assert!(diags.is_empty(), "expected clean parse, got {diags:?}");
    }

    #[test]
    fn rest_param_in_function_type_annotation_parses() {
        let (_, diags) =
            parse_str("let f: (a: number, ...rest: string[]) => void = (a, ...r) => {};");
        assert!(diags.is_empty(), "expected clean parse, got {diags:?}");
    }

    #[test]
    fn rest_param_not_last_rejected() {
        let (_, diags) = parse_str("function f(...xs: number[], y: number): void {}");
        assert!(
            diags.iter().any(|d| d
                .message
                .contains("rest parameter must be the last parameter")),
            "expected 'must be last' diagnostic, got {diags:?}",
        );
    }

    #[test]
    fn rest_param_with_default_rejected() {
        let (_, diags) = parse_str("function f(...xs: number[] = []): void {}");
        assert!(
            diags.iter().any(|d| d
                .message
                .contains("rest parameter cannot have a default value")),
            "expected default-rejection diagnostic, got {diags:?}",
        );
    }

    #[test]
    fn rest_param_without_annotation_rejected() {
        let (_, diags) = parse_str("function f(...xs): void {}");
        assert!(
            diags.iter().any(|d| d
                .message
                .contains("rest parameter requires a type annotation")),
            "expected missing-annotation diagnostic, got {diags:?}",
        );
    }

    #[test]
    fn rest_param_destructured_rejected() {
        let (_, diags) = parse_str("function f(...{a, b}): void {}");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("rest parameter cannot be destructured")),
            "expected destructured-rejection diagnostic, got {diags:?}",
        );
    }

    #[test]
    fn rest_param_in_function_type_with_non_array_rejected() {
        let (_, diags) = parse_str("let f: (...xs: number) => void = () => {};");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("rest parameter type must be an array")),
            "expected non-array-type diagnostic, got {diags:?}",
        );
    }

    fn call(e: &crate::Expr) -> (crate::ExprId, &[crate::ExprId]) {
        match e.kind {
            crate::ExprKind::Call {
                callee, ref args, ..
            } => (callee, args.as_slice()),
            _ => panic!("expected Call, got {:?}", e.kind),
        }
    }

    #[test]
    fn parse_call_no_args() {
        let (ast, diags) = parse_str("f();");
        assert!(diags.is_empty());
        let outer = expr_of_single_stmt(&ast);
        let (callee, args) = call(outer);
        assert!(matches!(
            ast.expr(callee).kind,
            crate::ExprKind::Identifier(_)
        ));
        assert!(args.is_empty());
        assert_eq!(outer.span, crate::Span::new(F, 0, 3));
    }

    #[test]
    fn parse_call_one_arg() {
        let (ast, diags) = parse_str("f(1);");
        assert!(diags.is_empty());
        let outer = expr_of_single_stmt(&ast);
        let (callee, args) = call(outer);
        assert!(matches!(
            ast.expr(callee).kind,
            crate::ExprKind::Identifier(_)
        ));
        assert_eq!(args.len(), 1);
        assert_eq!(ast.expr(args[0]).kind, crate::ExprKind::Number(1.0));
    }

    #[test]
    fn parse_call_three_args() {
        let (ast, diags) = parse_str("f(1, 2, 3);");
        assert!(diags.is_empty());
        let outer = expr_of_single_stmt(&ast);
        let (_callee, args) = call(outer);
        assert_eq!(args.len(), 3);
        assert_eq!(ast.expr(args[0]).kind, crate::ExprKind::Number(1.0));
        assert_eq!(ast.expr(args[1]).kind, crate::ExprKind::Number(2.0));
        assert_eq!(ast.expr(args[2]).kind, crate::ExprKind::Number(3.0));
    }

    #[test]
    fn parse_call_nested() {
        let (ast, diags) = parse_str("f(g(x));");
        assert!(diags.is_empty());
        let outer = expr_of_single_stmt(&ast);
        let (_callee, args) = call(outer);
        assert_eq!(args.len(), 1);
        let (_inner_callee, inner_args) = call(ast.expr(args[0]));
        assert_eq!(inner_args.len(), 1);
        assert!(matches!(
            ast.expr(inner_args[0]).kind,
            crate::ExprKind::Identifier(_)
        ));
    }

    #[test]
    fn parse_curried_call() {
        let (ast, diags) = parse_str("f()(x);");
        assert!(diags.is_empty());
        let outer = expr_of_single_stmt(&ast);
        let (callee, args) = call(outer);
        assert_eq!(args.len(), 1);
        let (_inner_callee, inner_args) = call(ast.expr(callee));
        assert!(inner_args.is_empty());
    }

    #[test]
    fn unary_binds_looser_than_call() {
        let (ast, diags) = parse_str("-f();");
        assert!(diags.is_empty());
        let outer = expr_of_single_stmt(&ast);
        let (op, operand) = unary(outer);
        assert_eq!(op, crate::UnOp::Neg);
        let (_callee, args) = call(ast.expr(operand));
        assert!(args.is_empty());
    }

    #[test]
    fn call_inside_binary() {
        let (ast, diags) = parse_str("f() + 1;");
        assert!(diags.is_empty());
        let outer = expr_of_single_stmt(&ast);
        let (op, lhs, rhs) = binary(outer);
        assert_eq!(op, crate::BinOp::Add);
        let (_callee, args) = call(ast.expr(lhs));
        assert!(args.is_empty());
        assert_eq!(ast.expr(rhs).kind, crate::ExprKind::Number(1.0));
    }

    #[test]
    fn call_with_complex_args() {
        let (ast, diags) = parse_str("f(1 + 2, g(x));");
        assert!(diags.is_empty());
        let outer = expr_of_single_stmt(&ast);
        let (_callee, args) = call(outer);
        assert_eq!(args.len(), 2);
        let (op, ..) = binary(ast.expr(args[0]));
        assert_eq!(op, crate::BinOp::Add);
        let (_, inner_args) = call(ast.expr(args[1]));
        assert_eq!(inner_args.len(), 1);
    }

    #[test]
    fn call_trailing_comma_ok() {
        let (ast, diags) = parse_str("f(1, 2,);");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let (_callee, args) = call(expr_of_single_stmt(&ast));
        assert_eq!(args.len(), 2);
    }

    #[test]
    fn unclosed_call_diagnoses() {
        let (ast, diags) = parse_str("f(1, 2");
        assert!(!diags.is_empty());
        assert!(diags[0].message == "expected `,` or `)`" || diags[0].message == "expected `)`");
        assert!(ast.top_level.is_empty());
    }

    #[test]
    fn call_expression_statement() {
        let (ast, diags) = parse_str(r#"greet("world");"#);
        assert!(diags.is_empty());
        let outer = expr_of_single_stmt(&ast);
        let (callee, args) = call(outer);
        assert!(matches!(
            ast.expr(callee).kind,
            crate::ExprKind::Identifier(_)
        ));
        assert_eq!(args.len(), 1);
        assert_eq!(
            ast.expr(args[0]).kind,
            crate::ExprKind::String("world".to_string())
        );
    }

    #[test]
    fn snapshot_multi_call_program() {
        let (ast, diags) = parse_str("f(); g(1); h(f(2), 3);");
        assert!(diags.is_empty());
        assert_eq!(ast.top_level.len(), 3);
        insta::assert_debug_snapshot!(ast);
    }

    #[test]
    fn parse_if_simple() {
        let (ast, diags) = parse_str("if (a) { }");
        assert!(diags.is_empty());
        let stmt = single_stmt(&ast);
        match stmt.kind {
            crate::StmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                assert!(matches!(
                    ast.expr(condition).kind,
                    crate::ExprKind::Identifier(_)
                ));
                assert!(matches!(
                    ast.stmt(then_block).kind,
                    crate::StmtKind::Block(ref v) if v.is_empty()
                ));
                assert!(else_block.is_none());
            }
            _ => panic!("expected If"),
        }
    }

    #[test]
    fn parse_if_else() {
        let (ast, diags) = parse_str("if (a) { } else { }");
        assert!(diags.is_empty());
        let stmt = single_stmt(&ast);
        match stmt.kind {
            crate::StmtKind::If { else_block, .. } => {
                let else_id = else_block.expect("expected else block");
                assert!(matches!(
                    ast.stmt(else_id).kind,
                    crate::StmtKind::Block(ref v) if v.is_empty()
                ));
            }
            _ => panic!("expected If"),
        }
    }

    #[test]
    fn parse_else_if_chain() {
        let (ast, diags) = parse_str("if (a) { } else if (b) { } else { }");
        assert!(diags.is_empty());
        let stmt = single_stmt(&ast);
        let crate::StmtKind::If {
            else_block: Some(inner_id),
            ..
        } = stmt.kind
        else {
            panic!("expected outer If with else_block");
        };
        let inner = ast.stmt(inner_id);
        let crate::StmtKind::If {
            else_block: Some(tail_id),
            ..
        } = inner.kind
        else {
            panic!("expected inner If for `else if`");
        };
        assert!(matches!(ast.stmt(tail_id).kind, crate::StmtKind::Block(_)));
    }

    #[test]
    fn parse_if_with_body_statements() {
        let (ast, diags) = parse_str("if (a) { let x = 1; }");
        assert!(diags.is_empty());
        let stmt = single_stmt(&ast);
        match stmt.kind {
            crate::StmtKind::If { then_block, .. } => {
                let crate::StmtKind::Block(ref body) = ast.stmt(then_block).kind else {
                    panic!("expected Block")
                };
                assert_eq!(body.len(), 1);
            }
            _ => panic!("expected If"),
        }
    }

    #[test]
    fn parse_if_braceless_body() {
        let (ast, diags) = parse_str("if (a) return b;");
        assert!(diags.is_empty(), "{diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::If {
            then_block,
            else_block,
            ..
        } = stmt.kind
        else {
            panic!("expected If");
        };
        let crate::StmtKind::Block(ref body) = ast.stmt(then_block).kind else {
            panic!("braceless body should wrap in a Block");
        };
        assert_eq!(body.len(), 1);
        assert!(matches!(ast.stmt(body[0]).kind, crate::StmtKind::Return(_)));
        assert!(else_block.is_none());
    }

    #[test]
    fn parse_braceless_dangling_else_binds_nearest_if() {
        // `else` attaches to the inner `if`, not the outer one.
        let (ast, diags) = parse_str("if (a) if (b) x(); else y();");
        assert!(diags.is_empty(), "{diags:?}");
        let crate::StmtKind::If {
            then_block,
            else_block: outer_else,
            ..
        } = single_stmt(&ast).kind
        else {
            panic!("expected outer If");
        };
        assert!(outer_else.is_none(), "else must bind to the inner if");
        let crate::StmtKind::Block(ref body) = ast.stmt(then_block).kind else {
            panic!("expected wrapped Block");
        };
        let crate::StmtKind::If {
            else_block: Some(_),
            ..
        } = ast.stmt(body[0]).kind
        else {
            panic!("inner If should own the else");
        };
    }

    #[test]
    fn parse_while_braceless_body() {
        let (ast, diags) = parse_str("while (a) x();");
        assert!(diags.is_empty(), "{diags:?}");
        let crate::StmtKind::While { body, .. } = single_stmt(&ast).kind else {
            panic!("expected While");
        };
        assert!(matches!(
            ast.stmt(body).kind,
            crate::StmtKind::Block(ref v) if v.len() == 1
        ));
    }

    #[test]
    fn parse_for_of_braceless_body() {
        let (ast, diags) = parse_str("for (const x of xs) f(x);");
        assert!(diags.is_empty(), "{diags:?}");
        let crate::StmtKind::ForOf { body, .. } = single_stmt(&ast).kind else {
            panic!("expected ForOf");
        };
        assert!(matches!(
            ast.stmt(body).kind,
            crate::StmtKind::Block(ref v) if v.len() == 1
        ));
    }

    #[test]
    fn parse_nested_if() {
        let (ast, diags) = parse_str("if (a) { if (b) { } }");
        assert!(diags.is_empty());
        let outer = single_stmt(&ast);
        let crate::StmtKind::If { then_block, .. } = outer.kind else {
            panic!("expected outer If");
        };
        let crate::StmtKind::Block(ref body) = ast.stmt(then_block).kind else {
            panic!("expected Block")
        };
        assert_eq!(body.len(), 1);
        assert!(matches!(ast.stmt(body[0]).kind, crate::StmtKind::If { .. }));
    }

    #[test]
    fn if_missing_open_paren_diagnoses() {
        let (ast, diags) = parse_str("if a { }");
        assert!(!diags.is_empty());
        assert_eq!(diags[0].message, "expected `(`");
        assert!(ast.top_level.is_empty());
    }

    #[test]
    fn if_missing_close_paren_diagnoses() {
        let (ast, diags) = parse_str("if (a { }");
        assert!(!diags.is_empty());
        assert_eq!(diags[0].message, "expected `)`");
        assert!(ast.top_level.is_empty());
    }

    #[test]
    fn if_braceless_body_parses() {
        let (ast, diags) = parse_str("if (a) x;");
        assert!(diags.is_empty(), "{diags:?}");
        let crate::StmtKind::If { then_block, .. } = single_stmt(&ast).kind else {
            panic!("expected If");
        };
        assert!(matches!(
            ast.stmt(then_block).kind,
            crate::StmtKind::Block(ref v) if v.len() == 1
        ));
    }

    #[test]
    fn snapshot_else_if_chain() {
        let (ast, diags) = parse_str("if (a) { } else if (b) { } else { }");
        assert!(diags.is_empty());
        insta::assert_debug_snapshot!(ast);
    }

    #[test]
    fn parse_while_simple() {
        let (ast, diags) = parse_str("while (a) { }");
        assert!(diags.is_empty());
        let stmt = single_stmt(&ast);
        match stmt.kind {
            crate::StmtKind::While { condition, body } => {
                assert!(matches!(
                    ast.expr(condition).kind,
                    crate::ExprKind::Identifier(_)
                ));
                assert!(matches!(
                    ast.stmt(body).kind,
                    crate::StmtKind::Block(ref v) if v.is_empty()
                ));
            }
            _ => panic!("expected While"),
        }
    }

    #[test]
    fn parse_while_with_body() {
        let (ast, diags) = parse_str("while (a) { x; y; }");
        assert!(diags.is_empty());
        let stmt = single_stmt(&ast);
        let crate::StmtKind::While { body, .. } = stmt.kind else {
            panic!("expected While");
        };
        let crate::StmtKind::Block(ref stmts) = ast.stmt(body).kind else {
            panic!("expected Block")
        };
        assert_eq!(stmts.len(), 2);
    }

    #[test]
    fn while_missing_paren_diagnoses() {
        let (ast, diags) = parse_str("while a { }");
        assert!(!diags.is_empty());
        assert_eq!(diags[0].message, "expected `(`");
        assert!(ast.top_level.is_empty());
    }

    #[test]
    fn parse_for_all_slots() {
        let (ast, diags) = parse_str("for (let i = 0; i < 10; i = i + 1) { x; }");
        assert!(diags.is_empty(), "{diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::For {
            init,
            condition,
            update,
            body,
        } = stmt.kind
        else {
            panic!("expected For");
        };
        assert!(init.is_some(), "init missing");
        assert!(condition.is_some(), "condition missing");
        assert!(update.is_some(), "update missing");
        assert!(matches!(ast.stmt(body).kind, crate::StmtKind::Block(_)));
    }

    #[test]
    fn parse_for_empty_slots() {
        let (ast, diags) = parse_str("for (;;) { break; }");
        assert!(diags.is_empty(), "{diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::For {
            init,
            condition,
            update,
            ..
        } = stmt.kind
        else {
            panic!("expected For");
        };
        assert!(init.is_none() && condition.is_none() && update.is_none());
    }

    #[test]
    fn parse_for_of_const_binding() {
        let (ast, diags) = parse_str("for (const x of arr) { y; }");
        assert!(diags.is_empty(), "{diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::ForOf {
            binding_kind,
            ref name,
            ref ty,
            ..
        } = stmt.kind
        else {
            panic!("expected ForOf");
        };
        assert!(matches!(binding_kind, crate::BindingKind::Const));
        assert_eq!(name.name, "x");
        assert!(ty.is_none());
    }

    #[test]
    fn parse_for_of_let_with_annotation() {
        let (ast, diags) = parse_str("for (let x: number of arr) { y; }");
        assert!(diags.is_empty(), "{diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::ForOf {
            binding_kind,
            ref ty,
            ..
        } = stmt.kind
        else {
            panic!("expected ForOf");
        };
        assert!(matches!(binding_kind, crate::BindingKind::Let));
        assert!(ty.is_some());
    }

    #[test]
    fn parse_do_while_simple() {
        let (ast, diags) = parse_str("do { x; } while (a);");
        assert!(diags.is_empty(), "{diags:?}");
        let stmt = single_stmt(&ast);
        assert!(matches!(stmt.kind, crate::StmtKind::DoWhile { .. }));
    }

    #[test]
    fn parse_break_and_continue() {
        let (ast, diags) = parse_str("while (a) { break; continue; }");
        assert!(diags.is_empty(), "{diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::While { body, .. } = stmt.kind else {
            panic!("expected While");
        };
        let crate::StmtKind::Block(ref stmts) = ast.stmt(body).kind else {
            panic!("expected Block");
        };
        assert!(matches!(ast.stmt(stmts[0]).kind, crate::StmtKind::Break));
        assert!(matches!(ast.stmt(stmts[1]).kind, crate::StmtKind::Continue));
    }

    #[test]
    fn parse_bare_return_in_function() {
        let (ast, diags) = parse_str("function f(): void { return; }");
        assert!(diags.is_empty());
        let f = single_stmt(&ast);
        let crate::StmtKind::Function { body, .. } = f.kind else {
            panic!("expected Function");
        };
        let crate::StmtKind::Block(ref stmts) = ast.stmt(body).kind else {
            panic!("expected Block")
        };
        assert_eq!(stmts.len(), 1);
        match ast.stmt(stmts[0]).kind {
            crate::StmtKind::Return(None) => {}
            _ => panic!("expected Return(None)"),
        }
    }

    #[test]
    fn parse_return_with_value() {
        let (ast, diags) = parse_str("function f(): number { return 1; }");
        assert!(diags.is_empty());
        let f = single_stmt(&ast);
        let crate::StmtKind::Function { body, .. } = f.kind else {
            panic!("expected Function");
        };
        let crate::StmtKind::Block(ref stmts) = ast.stmt(body).kind else {
            panic!("expected Block")
        };
        match ast.stmt(stmts[0]).kind {
            crate::StmtKind::Return(Some(value)) => {
                assert_eq!(ast.expr(value).kind, crate::ExprKind::Number(1.0));
            }
            _ => panic!("expected Return(Some)"),
        }
    }

    #[test]
    fn parse_return_with_expression() {
        let (ast, diags) = parse_str("function f(): number { return 1 + 2; }");
        assert!(diags.is_empty());
        let f = single_stmt(&ast);
        let crate::StmtKind::Function { body, .. } = f.kind else {
            panic!("expected Function");
        };
        let crate::StmtKind::Block(ref stmts) = ast.stmt(body).kind else {
            panic!("expected Block")
        };
        match ast.stmt(stmts[0]).kind {
            crate::StmtKind::Return(Some(value)) => {
                let (op, ..) = binary(ast.expr(value));
                assert_eq!(op, crate::BinOp::Add);
            }
            _ => panic!("expected Return(Some(Binary))"),
        }
    }

    #[test]
    fn return_missing_semicolon_diagnoses() {
        let (_, diags) = parse_str("function f(): number { return x x; }");
        assert!(!diags.is_empty());
        assert_eq!(diags[0].message, "expected `;` after return");
    }

    #[test]
    fn parse_bare_block() {
        let (ast, diags) = parse_str("{ x; y; }");
        assert!(diags.is_empty());
        let stmt = single_stmt(&ast);
        let crate::StmtKind::Block(ref stmts) = stmt.kind else {
            panic!("expected Block");
        };
        assert_eq!(stmts.len(), 2);
    }

    #[test]
    fn parse_empty_bare_block() {
        let (ast, diags) = parse_str("{ }");
        assert!(diags.is_empty());
        let stmt = single_stmt(&ast);
        let crate::StmtKind::Block(ref stmts) = stmt.kind else {
            panic!("expected Block");
        };
        assert!(stmts.is_empty());
    }

    #[test]
    fn parse_nested_bare_blocks() {
        let (ast, diags) = parse_str("{ { } }");
        assert!(diags.is_empty());
        let outer = single_stmt(&ast);
        let crate::StmtKind::Block(ref stmts) = outer.kind else {
            panic!("expected outer Block");
        };
        assert_eq!(stmts.len(), 1);
        assert!(matches!(
            ast.stmt(stmts[0]).kind,
            crate::StmtKind::Block(ref v) if v.is_empty()
        ));
    }

    #[test]
    fn snapshot_function_with_if_and_return() {
        let source = "function isPositive(n: number): boolean {\n  if (n > 0) {\n    return true;\n  } else {\n    return false;\n  }\n}";
        let (ast, diags) = parse_str(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        insta::assert_debug_snapshot!(ast);
    }

    fn object_literal(e: &crate::Expr) -> &[crate::ObjectLiteralMember] {
        match e.kind {
            crate::ExprKind::ObjectLiteral { ref members } => members.as_slice(),
            _ => panic!("expected ObjectLiteral, got {:?}", e.kind),
        }
    }

    fn object_literal_field(m: &crate::ObjectLiteralMember) -> &crate::ObjectLiteralField {
        match m {
            crate::ObjectLiteralMember::Field(f) => f,
            crate::ObjectLiteralMember::Spread { .. } => {
                panic!("expected Field member, got Spread")
            }
        }
    }

    fn array_literal(e: &crate::Expr) -> &[crate::ArrayLiteralElement] {
        match e.kind {
            crate::ExprKind::ArrayLiteral { ref elements } => elements.as_slice(),
            _ => panic!("expected ArrayLiteral, got {:?}", e.kind),
        }
    }

    fn array_literal_value(el: &crate::ArrayLiteralElement) -> crate::ExprId {
        match el {
            crate::ArrayLiteralElement::Value(id) => *id,
            crate::ArrayLiteralElement::Spread { .. } => {
                panic!("expected Value element, got Spread")
            }
        }
    }

    #[test]
    fn parse_empty_object_literal() {
        let (ast, diags) = parse_str("let x = {};");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let crate::StmtKind::Let { value, .. } = single_stmt(&ast).kind else {
            panic!("expected Let");
        };
        assert!(object_literal(ast.expr(value)).is_empty());
    }

    #[test]
    fn parse_object_literal_simple() {
        let (ast, diags) = parse_str("let p = { x: 1, y: 2 };");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let crate::StmtKind::Let { value, .. } = single_stmt(&ast).kind else {
            panic!("expected Let");
        };
        let members = object_literal(ast.expr(value));
        assert_eq!(members.len(), 2);
        let f0 = object_literal_field(&members[0]);
        let f1 = object_literal_field(&members[1]);
        assert_eq!(f0.name.name, "x");
        assert_eq!(f1.name.name, "y");
        assert_eq!(ast.expr(f0.value).kind, crate::ExprKind::Number(1.0));
        assert_eq!(ast.expr(f1.value).kind, crate::ExprKind::Number(2.0));
    }

    #[test]
    fn parse_object_literal_keyword_keys() {
        let (ast, diags) = parse_str("let p = { type: 1, default: 2, null: 3, true: 4 };");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let crate::StmtKind::Let { value, .. } = single_stmt(&ast).kind else {
            panic!("expected Let");
        };
        let names: Vec<&str> = object_literal(ast.expr(value))
            .iter()
            .map(object_literal_field)
            .map(|f| f.name.name.as_str())
            .collect();
        assert_eq!(names, vec!["type", "default", "null", "true"]);
    }

    #[test]
    fn parse_object_literal_string_key() {
        let (ast, diags) = parse_str(r#"let p = { "hello": 1 };"#);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let crate::StmtKind::Let { value, .. } = single_stmt(&ast).kind else {
            panic!("expected Let");
        };
        let members = object_literal(ast.expr(value));
        assert_eq!(members.len(), 1);
        let f0 = object_literal_field(&members[0]);
        assert_eq!(f0.name.name, "hello");
    }

    #[test]
    fn parse_object_literal_string_key_decodes_escape() {
        let (ast, diags) = parse_str(r#"let p = { "content\u002dtype": 1 };"#);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let crate::StmtKind::Let { value, .. } = single_stmt(&ast).kind else {
            panic!("expected Let");
        };
        let members = object_literal(ast.expr(value));
        assert_eq!(members.len(), 1);
        let f0 = object_literal_field(&members[0]);
        assert_eq!(f0.name.name, "content-type");
    }

    #[test]
    fn parse_object_literal_shorthand() {
        let (ast, diags) = parse_str("let p = { x, y: 2 };");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let crate::StmtKind::Let { value, .. } = single_stmt(&ast).kind else {
            panic!("expected Let");
        };
        let members = object_literal(ast.expr(value));
        assert_eq!(members.len(), 2);
        let f0 = object_literal_field(&members[0]);
        assert_eq!(f0.name.name, "x");
        // `{ x }` desugars to `{ x: x }` — value is an identifier reference.
        assert_eq!(
            ast.expr(f0.value).kind,
            crate::ExprKind::Identifier(crate::Ident {
                name: "x".into(),
                span: f0.name.span,
            })
        );
        let f1 = object_literal_field(&members[1]);
        assert_eq!(f1.name.name, "y");
        assert_eq!(ast.expr(f1.value).kind, crate::ExprKind::Number(2.0));
    }

    #[test]
    fn parse_object_literal_string_key_shorthand_rejected() {
        let (_ast, diags) = parse_str(r#"let p = { "x" };"#);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("expected `:` after field name")),
            "string-key shorthand should be rejected: {diags:?}"
        );
    }

    #[test]
    fn parse_object_literal_trailing_comma() {
        let (ast, diags) = parse_str("let p = { x: 1, y: 2, };");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let crate::StmtKind::Let { value, .. } = single_stmt(&ast).kind else {
            panic!("expected Let");
        };
        assert_eq!(object_literal(ast.expr(value)).len(), 2);
    }

    #[test]
    fn parse_object_literal_duplicate_key_diagnoses() {
        let (_, diags) = parse_str("let p = { x: 1, x: 2 };");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "duplicate field `x` in object literal");
    }

    #[test]
    fn parse_empty_array_literal() {
        let (ast, diags) = parse_str("let xs = [];");
        assert!(diags.is_empty());
        let crate::StmtKind::Let { value, .. } = single_stmt(&ast).kind else {
            panic!("expected Let");
        };
        assert!(array_literal(ast.expr(value)).is_empty());
    }

    #[test]
    fn parse_array_literal_simple() {
        let (ast, diags) = parse_str("let xs = [1, 2, 3];");
        assert!(diags.is_empty());
        let crate::StmtKind::Let { value, .. } = single_stmt(&ast).kind else {
            panic!("expected Let");
        };
        let elements = array_literal(ast.expr(value));
        assert_eq!(elements.len(), 3);
        assert_eq!(
            ast.expr(array_literal_value(&elements[0])).kind,
            crate::ExprKind::Number(1.0)
        );
        assert_eq!(
            ast.expr(array_literal_value(&elements[2])).kind,
            crate::ExprKind::Number(3.0)
        );
    }

    #[test]
    fn parse_array_literal_trailing_comma() {
        let (ast, diags) = parse_str("let xs = [1, 2,];");
        assert!(diags.is_empty());
        let crate::StmtKind::Let { value, .. } = single_stmt(&ast).kind else {
            panic!("expected Let");
        };
        assert_eq!(array_literal(ast.expr(value)).len(), 2);
    }

    #[test]
    fn parse_nested_array_in_object() {
        let (ast, diags) = parse_str("let p = { items: [1, 2] };");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let crate::StmtKind::Let { value, .. } = single_stmt(&ast).kind else {
            panic!("expected Let");
        };
        let members = object_literal(ast.expr(value));
        let f0 = object_literal_field(&members[0]);
        assert_eq!(f0.name.name, "items");
        let inner = array_literal(ast.expr(f0.value));
        assert_eq!(inner.len(), 2);
    }

    #[test]
    fn parse_nested_object_in_array() {
        let (ast, diags) = parse_str("let xs = [{ x: 1 }, { x: 2 }];");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let crate::StmtKind::Let { value, .. } = single_stmt(&ast).kind else {
            panic!("expected Let");
        };
        let elements = array_literal(ast.expr(value));
        assert_eq!(elements.len(), 2);
        assert_eq!(
            object_literal(ast.expr(array_literal_value(&elements[0]))).len(),
            1
        );
        assert_eq!(
            object_literal(ast.expr(array_literal_value(&elements[1]))).len(),
            1
        );
    }

    #[test]
    fn brace_at_statement_start_is_block_not_object() {
        let (_, diags) = parse_str("{ x: 1 }");
        assert!(
            !diags.is_empty(),
            "expected a diagnostic for `x:` inside block"
        );
    }

    fn field_access(e: &crate::Expr) -> (crate::ExprId, &str) {
        match e.kind {
            crate::ExprKind::FieldAccess { receiver, ref name } => (receiver, name.name.as_str()),
            _ => panic!("expected FieldAccess, got {:?}", e.kind),
        }
    }

    fn index_access(e: &crate::Expr) -> (crate::ExprId, crate::ExprId) {
        match e.kind {
            crate::ExprKind::IndexAccess { receiver, index } => (receiver, index),
            _ => panic!("expected IndexAccess, got {:?}", e.kind),
        }
    }

    #[test]
    fn parse_field_access_simple() {
        let (ast, diags) = parse_str("a.b;");
        assert!(diags.is_empty());
        let outer = expr_of_single_stmt(&ast);
        let (receiver, name) = field_access(outer);
        assert!(matches!(
            ast.expr(receiver).kind,
            crate::ExprKind::Identifier(_)
        ));
        assert_eq!(name, "b");
    }

    #[test]
    fn parse_field_access_keyword_name() {
        let (ast, diags) = parse_str("status.type.default.null;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let outer = expr_of_single_stmt(&ast);
        let (receiver, name) = field_access(outer);
        assert_eq!(name, "null");
        let (receiver, name) = field_access(ast.expr(receiver));
        assert_eq!(name, "default");
        let (_receiver, name) = field_access(ast.expr(receiver));
        assert_eq!(name, "type");
    }

    #[test]
    fn parse_field_access_chain_left_assoc() {
        let (ast, diags) = parse_str("a.b.c;");
        assert!(diags.is_empty());
        let outer = expr_of_single_stmt(&ast);
        let (mid_id, c) = field_access(outer);
        assert_eq!(c, "c");
        let (a_id, b) = field_access(ast.expr(mid_id));
        assert_eq!(b, "b");
        assert!(matches!(
            ast.expr(a_id).kind,
            crate::ExprKind::Identifier(_)
        ));
    }

    #[test]
    fn parse_index_access_simple() {
        let (ast, diags) = parse_str("a[0];");
        assert!(diags.is_empty());
        let outer = expr_of_single_stmt(&ast);
        let (receiver, idx) = index_access(outer);
        assert!(matches!(
            ast.expr(receiver).kind,
            crate::ExprKind::Identifier(_)
        ));
        assert_eq!(ast.expr(idx).kind, crate::ExprKind::Number(0.0));
    }

    #[test]
    fn parse_index_access_chain() {
        let (ast, diags) = parse_str("a[0][1];");
        assert!(diags.is_empty());
        let outer = expr_of_single_stmt(&ast);
        let (mid_id, one) = index_access(outer);
        assert_eq!(ast.expr(one).kind, crate::ExprKind::Number(1.0));
        let (a_id, zero) = index_access(ast.expr(mid_id));
        assert_eq!(ast.expr(zero).kind, crate::ExprKind::Number(0.0));
        assert!(matches!(
            ast.expr(a_id).kind,
            crate::ExprKind::Identifier(_)
        ));
    }

    #[test]
    fn parse_mixed_access_and_call_chain() {
        let (ast, diags) = parse_str("a.b[c].d();");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let outer = expr_of_single_stmt(&ast);
        let (callee, args) = call(outer);
        assert!(args.is_empty());
        let (idx_id, d_name) = field_access(ast.expr(callee));
        assert_eq!(d_name, "d");
        let (b_id, c_idx) = index_access(ast.expr(idx_id));
        assert!(matches!(
            ast.expr(c_idx).kind,
            crate::ExprKind::Identifier(_)
        ));
        let (a_id, b_name) = field_access(ast.expr(b_id));
        assert_eq!(b_name, "b");
        assert!(matches!(
            ast.expr(a_id).kind,
            crate::ExprKind::Identifier(_)
        ));
    }

    #[test]
    fn parse_call_then_field() {
        let (ast, diags) = parse_str("f().x;");
        assert!(diags.is_empty());
        let outer = expr_of_single_stmt(&ast);
        let (call_id, x_name) = field_access(outer);
        assert_eq!(x_name, "x");
        let (_callee, args) = call(ast.expr(call_id));
        assert!(args.is_empty());
    }

    #[test]
    fn parse_index_with_complex_expression() {
        let (ast, diags) = parse_str("a[i + 1];");
        assert!(diags.is_empty());
        let outer = expr_of_single_stmt(&ast);
        let (_, idx) = index_access(outer);
        let (op, ..) = binary(ast.expr(idx));
        assert_eq!(op, crate::BinOp::Add);
    }

    #[test]
    fn field_access_missing_name_diagnoses() {
        let (_, diags) = parse_str("a.;");
        assert!(!diags.is_empty());
        assert_eq!(diags[0].message, "expected field name after `.`");
    }

    #[test]
    fn parse_postfix_increment() {
        let (ast, diags) = parse_str("x++;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let expr = expr_of_single_stmt(&ast);
        let crate::ExprKind::PostfixUnary { op, operand } = expr.kind else {
            panic!("expected PostfixUnary, got {:?}", expr.kind);
        };
        assert_eq!(op, crate::PostfixOp::Inc);
        assert!(matches!(
            ast.expr(operand).kind,
            crate::ExprKind::Identifier(_)
        ));
        assert_eq!(expr.span, crate::Span::new(F, 0, 3));
    }

    #[test]
    fn parse_postfix_decrement() {
        let (ast, diags) = parse_str("x--;");
        assert!(diags.is_empty());
        let expr = expr_of_single_stmt(&ast);
        let crate::ExprKind::PostfixUnary { op, .. } = expr.kind else {
            panic!("expected PostfixUnary");
        };
        assert_eq!(op, crate::PostfixOp::Dec);
    }

    #[test]
    fn parse_postfix_non_null_assertion() {
        let (ast, diags) = parse_str("x!;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let expr = expr_of_single_stmt(&ast);
        let crate::ExprKind::PostfixUnary { op, operand } = expr.kind else {
            panic!("expected PostfixUnary, got {:?}", expr.kind);
        };
        assert_eq!(op, crate::PostfixOp::NonNullAssert);
        assert!(matches!(
            ast.expr(operand).kind,
            crate::ExprKind::Identifier(_)
        ));
        assert_eq!(expr.span, crate::Span::new(F, 0, 2));
    }

    #[test]
    fn parse_non_null_assertion_continues_chain() {
        let (ast, diags) = parse_str("x!.y;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let expr = expr_of_single_stmt(&ast);
        let crate::ExprKind::FieldAccess { receiver, name } = &expr.kind else {
            panic!("expected FieldAccess, got {:?}", expr.kind);
        };
        assert_eq!(name.name, "y");
        assert!(matches!(
            ast.expr(*receiver).kind,
            crate::ExprKind::PostfixUnary {
                op: crate::PostfixOp::NonNullAssert,
                ..
            }
        ));
    }

    #[test]
    fn parse_postfix_on_field_access() {
        let (ast, diags) = parse_str("o.f++;");
        assert!(diags.is_empty());
        let expr = expr_of_single_stmt(&ast);
        let crate::ExprKind::PostfixUnary { operand, .. } = expr.kind else {
            panic!("expected PostfixUnary");
        };
        assert!(matches!(
            ast.expr(operand).kind,
            crate::ExprKind::FieldAccess { .. }
        ));
    }

    #[test]
    fn parse_postfix_on_index_access() {
        let (ast, diags) = parse_str("xs[0]++;");
        assert!(diags.is_empty());
        let expr = expr_of_single_stmt(&ast);
        let crate::ExprKind::PostfixUnary { operand, .. } = expr.kind else {
            panic!("expected PostfixUnary");
        };
        assert!(matches!(
            ast.expr(operand).kind,
            crate::ExprKind::IndexAccess { .. }
        ));
    }

    #[test]
    fn parse_postfix_terminates_chain() {
        let (_, diags) = parse_str("x++.y;");
        assert!(!diags.is_empty());
        assert_eq!(diags[0].message, "expected `;` after expression");
    }

    #[test]
    fn parse_postfix_in_binary_continues() {
        let (ast, diags) = parse_str("let y = x++ + 1;");
        assert!(diags.is_empty(), "unexpected: {diags:?}");
        let crate::StmtKind::Let { value, .. } = single_stmt(&ast).kind else {
            panic!("expected Let");
        };
        let (op, lhs, _) = binary(ast.expr(value));
        assert_eq!(op, crate::BinOp::Add);
        assert!(matches!(
            ast.expr(lhs).kind,
            crate::ExprKind::PostfixUnary { .. }
        ));
    }

    #[test]
    fn parse_index_assignment() {
        let (ast, diags) = parse_str("xs[0] = 1;");
        assert!(diags.is_empty(), "unexpected: {diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::AssignIndex {
            receiver,
            index,
            value,
        } = stmt.kind
        else {
            panic!("expected AssignIndex, got {:?}", stmt.kind);
        };
        assert!(matches!(
            ast.expr(receiver).kind,
            crate::ExprKind::Identifier(_)
        ));
        assert_eq!(ast.expr(index).kind, crate::ExprKind::Number(0.0));
        assert_eq!(ast.expr(value).kind, crate::ExprKind::Number(1.0));
    }

    #[test]
    fn parse_for_update_postfix() {
        let (ast, diags) = parse_str("function main(): void { for (let i = 0; i < 3; i++) {} }");
        assert!(diags.is_empty(), "unexpected: {diags:?}");
        assert_eq!(ast.top_level.len(), 1);
    }

    #[test]
    fn parse_for_update_index_assignment() {
        let (ast, diags) =
            parse_str("function main(): void { for (let i = 0; i < 3; xs[i] = i) {} }");
        assert!(diags.is_empty(), "unexpected: {diags:?}");
        assert_eq!(ast.top_level.len(), 1);
    }

    #[test]
    fn index_access_missing_close_bracket_diagnoses() {
        let (_, diags) = parse_str("a[1;");
        assert!(!diags.is_empty());
        assert_eq!(diags[0].message, "expected `]`");
    }

    #[test]
    fn field_access_inside_object_literal_value_does_not_steal_dot() {
        let (ast, diags) = parse_str("let x = { a: 1 }.foo;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let crate::StmtKind::Let { value, .. } = single_stmt(&ast).kind else {
            panic!("expected Let");
        };
        let (recv_id, name) = field_access(ast.expr(value));
        assert_eq!(name, "foo");
        assert_eq!(object_literal(ast.expr(recv_id)).len(), 1);
    }

    fn type_of_let(stmt: &crate::Stmt) -> &crate::TypeAnnotation {
        match stmt.kind {
            crate::StmtKind::Let { ref ty, .. } => ty.as_ref().expect("expected type annotation"),
            _ => panic!("expected Let"),
        }
    }

    #[test]
    fn parse_array_type_annotation() {
        let (ast, diags) = parse_str("let xs: number[] = null;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let ty = type_of_let(single_stmt(&ast));
        match ty.kind {
            crate::TypeAnnotationKind::Array(ref inner) => {
                assert!(matches!(inner.kind, crate::TypeAnnotationKind::Name { .. }));
            }
            _ => panic!("expected Array, got {:?}", ty.kind),
        }
    }

    #[test]
    fn parse_nested_array_type_annotation() {
        let (ast, diags) = parse_str("let xs: number[][] = null;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let ty = type_of_let(single_stmt(&ast));
        let crate::TypeAnnotationKind::Array(ref inner) = ty.kind else {
            panic!("expected outer Array");
        };
        let crate::TypeAnnotationKind::Array(ref inner2) = inner.kind else {
            panic!("expected inner Array");
        };
        assert!(matches!(
            inner2.kind,
            crate::TypeAnnotationKind::Name { .. }
        ));
    }

    #[test]
    fn parse_object_type_annotation_simple() {
        let (ast, diags) = parse_str("let p: { x: number; y: number } = null;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let ty = type_of_let(single_stmt(&ast));
        let crate::TypeAnnotationKind::Object { ref fields } = ty.kind else {
            panic!("expected Object, got {:?}", ty.kind);
        };
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name.name, "x");
        assert_eq!(fields[1].name.name, "y");
    }

    #[test]
    fn parse_object_type_method_members() {
        // `m(): T` is the same member as `m: () => T`, so it parses to a function-typed
        // field — including through the `?` and `readonly` modifiers.
        let (ast, diags) = parse_str(
            "let p: { m(): number; opt?(): string; readonly r(a: number): boolean } = null;",
        );
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let ty = type_of_let(single_stmt(&ast));
        let crate::TypeAnnotationKind::Object { ref fields } = ty.kind else {
            panic!("expected Object, got {:?}", ty.kind);
        };
        assert_eq!(fields.len(), 3);
        for field in fields {
            assert!(
                matches!(field.ty.kind, crate::TypeAnnotationKind::Function { .. }),
                "member `{}` should be function-typed, got {:?}",
                field.name.name,
                field.ty.kind,
            );
        }
        assert!(fields[1].optional, "`opt?()` is an optional member");
        assert!(fields[2].readonly, "`readonly r()` keeps its modifier");
        let crate::TypeAnnotationKind::Function { ref params, .. } = fields[2].ty.kind else {
            unreachable!("checked above");
        };
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name.name, "a");
    }

    #[test]
    fn parse_object_type_annotation_with_commas() {
        let (ast, diags) = parse_str("let p: { x: number, y: number } = null;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let ty = type_of_let(single_stmt(&ast));
        let crate::TypeAnnotationKind::Object { ref fields } = ty.kind else {
            panic!("expected Object");
        };
        assert_eq!(fields.len(), 2);
    }

    #[test]
    fn parse_object_type_annotation_empty() {
        let (ast, diags) = parse_str("let p: {} = null;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let ty = type_of_let(single_stmt(&ast));
        let crate::TypeAnnotationKind::Object { ref fields } = ty.kind else {
            panic!("expected Object");
        };
        assert!(fields.is_empty());
    }

    #[test]
    fn parse_object_type_with_optional_field() {
        let (ast, diags) = parse_str("let p: { id: number; nick?: string } = null;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let ty = type_of_let(single_stmt(&ast));
        let crate::TypeAnnotationKind::Object { ref fields } = ty.kind else {
            panic!("expected Object");
        };
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name.name, "id");
        assert!(!fields[0].optional);
        assert_eq!(fields[1].name.name, "nick");
        assert!(fields[1].optional);
    }

    #[test]
    fn parse_object_type_with_readonly_fields() {
        let (ast, diags) = parse_str(
            "let issue: { readonly id: string; readonly title?: string; body: string } = null;",
        );
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let ty = type_of_let(single_stmt(&ast));
        let crate::TypeAnnotationKind::Object { ref fields } = ty.kind else {
            panic!("expected Object");
        };
        let fields: Vec<(&str, bool, bool)> = fields
            .iter()
            .map(|field| (field.name.name.as_str(), field.optional, field.readonly))
            .collect();
        assert_eq!(
            fields,
            vec![
                ("id", false, true),
                ("title", true, true),
                ("body", false, false),
            ]
        );
    }

    #[test]
    fn parse_object_type_mixed_optional_and_required() {
        let (ast, diags) = parse_str("let p: { a?: number; b: string; c?: boolean } = null;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let ty = type_of_let(single_stmt(&ast));
        let crate::TypeAnnotationKind::Object { ref fields } = ty.kind else {
            panic!("expected Object");
        };
        let opt: Vec<(&str, bool)> = fields
            .iter()
            .map(|f| (f.name.name.as_str(), f.optional))
            .collect();
        assert_eq!(opt, vec![("a", true), ("b", false), ("c", true)]);
    }

    #[test]
    fn parse_object_type_keyword_fields() {
        let (ast, diags) =
            parse_str("let p: { type: string; default?: number; null: boolean } = null;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let ty = type_of_let(single_stmt(&ast));
        let crate::TypeAnnotationKind::Object { ref fields } = ty.kind else {
            panic!("expected Object");
        };
        let opt: Vec<(&str, bool)> = fields
            .iter()
            .map(|f| (f.name.name.as_str(), f.optional))
            .collect();
        assert_eq!(
            opt,
            vec![("type", false), ("default", true), ("null", false)]
        );
    }

    #[test]
    fn parse_object_type_quoted_fields() {
        let (ast, diags) =
            parse_str(r#"let p: { "content-type": string; "x\u002drequest-id"?: string } = null;"#);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let ty = type_of_let(single_stmt(&ast));
        let crate::TypeAnnotationKind::Object { ref fields } = ty.kind else {
            panic!("expected Object");
        };
        let opt: Vec<(&str, bool)> = fields
            .iter()
            .map(|f| (f.name.name.as_str(), f.optional))
            .collect();
        assert_eq!(opt, vec![("content-type", false), ("x-request-id", true)]);
    }

    #[test]
    fn parse_interface_with_optional_property() {
        let (ast, diags) = parse_str("interface User { id: number; deletedAt?: string; }");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::InterfaceDecl { ref members, .. } = stmt.kind else {
            panic!("expected interface decl");
        };
        let opt: Vec<(&str, bool)> = members
            .iter()
            .map(|m| match m {
                crate::InterfaceMember::Property { name, optional, .. } => {
                    (name.name.as_str(), *optional)
                }
                _ => panic!("expected property"),
            })
            .collect();
        assert_eq!(opt, vec![("id", false), ("deletedAt", true)]);
    }

    #[test]
    fn parse_interface_with_readonly_properties() {
        let (ast, diags) = parse_str(
            "interface Issue { readonly id: string; readonly title?: string; body: string; }",
        );
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::InterfaceDecl { ref members, .. } = stmt.kind else {
            panic!("expected interface decl");
        };
        let opt: Vec<(&str, bool, bool)> = members
            .iter()
            .map(|member| match member {
                crate::InterfaceMember::Property {
                    name,
                    optional,
                    readonly,
                    ..
                } => (name.name.as_str(), *optional, *readonly),
                _ => panic!("expected property"),
            })
            .collect();
        assert_eq!(
            opt,
            vec![
                ("id", false, true),
                ("title", true, true),
                ("body", false, false),
            ]
        );
    }

    #[test]
    fn parse_readonly_property_name_without_modifier() {
        let (ast, diags) = parse_str(
            "interface Meta { readonly: boolean; }\nlet meta: { readonly: boolean } = null;",
        );
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let crate::StmtKind::InterfaceDecl { ref members, .. } = ast.stmt(ast.top_level[0]).kind
        else {
            panic!("expected interface decl");
        };
        let crate::InterfaceMember::Property {
            ref name, readonly, ..
        } = members[0]
        else {
            panic!("expected property");
        };
        assert_eq!(name.name, "readonly");
        assert!(
            !readonly,
            "`readonly` here is the property name, not a modifier"
        );

        let ty = type_of_let(ast.stmt(ast.top_level[1]));
        let crate::TypeAnnotationKind::Object { ref fields } = ty.kind else {
            panic!("expected Object");
        };
        assert_eq!(fields[0].name.name, "readonly");
        assert!(!fields[0].readonly);
    }

    #[test]
    fn parse_interface_keyword_members() {
        let (ast, diags) =
            parse_str("interface IssueStatus { type: string; default(): string; null: boolean; }");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::InterfaceDecl { ref members, .. } = stmt.kind else {
            panic!("expected interface decl");
        };
        let names: Vec<&str> = members
            .iter()
            .map(|m| match m {
                crate::InterfaceMember::Method { name, .. }
                | crate::InterfaceMember::Property { name, .. } => name.name.as_str(),
            })
            .collect();
        assert_eq!(names, vec!["type", "default", "null"]);
    }

    #[test]
    fn parse_interface_quoted_property_members() {
        let (ast, diags) = parse_str(
            r#"interface Headers { "content-type": string; "x\u002drequest-id"?: string; }"#,
        );
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::InterfaceDecl { ref members, .. } = stmt.kind else {
            panic!("expected interface decl");
        };
        let names: Vec<(&str, bool)> = members
            .iter()
            .map(|m| match m {
                crate::InterfaceMember::Property { name, optional, .. } => {
                    (name.name.as_str(), *optional)
                }
                _ => panic!("expected property"),
            })
            .collect();
        assert_eq!(names, vec![("content-type", false), ("x-request-id", true)]);
    }

    #[test]
    fn parse_param_rejects_optional_marker() {
        let (_ast, diags) = parse_str("function f(x?: number): void {}");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("optional function parameters")),
            "expected optional-param diagnostic, got {diags:?}"
        );
    }

    #[test]
    fn parse_interface_rejects_optional_method() {
        let (_ast, diags) = parse_str("interface F { foo?(): void; }");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("optional interface methods")),
            "expected optional-method diagnostic, got {diags:?}"
        );
    }

    #[test]
    fn parse_interface_with_call_signature() {
        let (ast, diags) = parse_str("interface F { (s: string): number; }");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::InterfaceDecl { ref members, .. } = stmt.kind else {
            panic!("expected interface decl");
        };
        assert_eq!(members.len(), 1);
        match &members[0] {
            crate::InterfaceMember::Method { name, params, .. } => {
                assert_eq!(name.name, "@call");
                assert_eq!(params.len(), 1);
                assert_eq!(params[0].name.name, "s");
            }
            other => panic!("expected method (call signature), got {other:?}"),
        }
    }

    #[test]
    fn parse_interface_mixed_call_signature_and_method() {
        let (ast, diags) = parse_str("interface F { greet(): string; (x: number): number; }");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::InterfaceDecl { ref members, .. } = stmt.kind else {
            panic!("expected interface decl");
        };
        let names: Vec<&str> = members
            .iter()
            .map(|m| match m {
                crate::InterfaceMember::Method { name, .. } => name.name.as_str(),
                _ => "<property>",
            })
            .collect();
        assert_eq!(names, vec!["greet", "@call"]);
    }

    #[test]
    fn parse_interface_rejects_generic_call_signature() {
        let (_ast, diags) = parse_str("interface F { <T>(x: T): T; }");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("generic call signatures")),
            "expected generic-call-signature diagnostic, got {diags:?}"
        );
    }

    #[test]
    fn parse_object_type_with_array_field() {
        let (ast, diags) = parse_str("let p: { xs: number[] } = null;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let ty = type_of_let(single_stmt(&ast));
        let crate::TypeAnnotationKind::Object { ref fields } = ty.kind else {
            panic!("expected Object");
        };
        assert_eq!(fields[0].name.name, "xs");
        assert!(matches!(
            fields[0].ty.kind,
            crate::TypeAnnotationKind::Array(_)
        ));
    }

    #[test]
    fn parse_array_of_objects() {
        let (ast, diags) = parse_str("let xs: { x: number }[] = null;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let ty = type_of_let(single_stmt(&ast));
        let crate::TypeAnnotationKind::Array(ref inner) = ty.kind else {
            panic!("expected Array");
        };
        assert!(matches!(
            inner.kind,
            crate::TypeAnnotationKind::Object { .. }
        ));
    }

    #[test]
    fn parse_tuple_type_annotation() {
        let (ast, diags) = parse_str("let p: [string, number] = null;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let ty = type_of_let(single_stmt(&ast));
        let crate::TypeAnnotationKind::Tuple(ref elems) = ty.kind else {
            panic!("expected Tuple, got {:?}", ty.kind);
        };
        assert_eq!(elems.len(), 2);
        assert!(matches!(
            elems[0].kind,
            crate::TypeAnnotationKind::Name { .. }
        ));
        assert!(matches!(
            elems[1].kind,
            crate::TypeAnnotationKind::Name { .. }
        ));
    }

    #[test]
    fn parse_singleton_tuple_type() {
        let (ast, diags) = parse_str("let p: [number] = null;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let ty = type_of_let(single_stmt(&ast));
        let crate::TypeAnnotationKind::Tuple(ref elems) = ty.kind else {
            panic!("expected Tuple");
        };
        assert_eq!(elems.len(), 1);
    }

    #[test]
    fn parse_nested_tuple_type() {
        let (ast, diags) = parse_str("let p: [[number, string], boolean] = null;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let ty = type_of_let(single_stmt(&ast));
        let crate::TypeAnnotationKind::Tuple(ref outer) = ty.kind else {
            panic!("expected outer Tuple");
        };
        assert_eq!(outer.len(), 2);
        let crate::TypeAnnotationKind::Tuple(ref inner) = outer[0].kind else {
            panic!("expected inner Tuple");
        };
        assert_eq!(inner.len(), 2);
    }

    #[test]
    fn parse_array_of_tuple_type() {
        let (ast, diags) = parse_str("let xs: [string, number][] = null;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let ty = type_of_let(single_stmt(&ast));
        let crate::TypeAnnotationKind::Array(ref inner) = ty.kind else {
            panic!("expected outer Array, got {:?}", ty.kind);
        };
        assert!(matches!(inner.kind, crate::TypeAnnotationKind::Tuple(_)));
    }

    #[test]
    fn parse_trailing_comma_tuple_type() {
        let (ast, diags) = parse_str("let p: [number, string,] = null;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let ty = type_of_let(single_stmt(&ast));
        let crate::TypeAnnotationKind::Tuple(ref elems) = ty.kind else {
            panic!("expected Tuple");
        };
        assert_eq!(elems.len(), 2);
    }

    #[test]
    fn parse_empty_tuple_type_rejected() {
        let (_ast, diags) = parse_str("let p: [] = null;");
        assert!(
            diags.iter().any(|d| d
                .message
                .contains("tuple types must have at least one element")),
            "expected empty-tuple diagnostic, got: {diags:?}"
        );
    }

    #[test]
    fn parse_generic_one_arg() {
        let (ast, diags) = parse_str("let x: Foo<number> = null;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let ty = type_of_let(single_stmt(&ast));
        let crate::TypeAnnotationKind::Name {
            name_span,
            ref args,
        } = ty.kind
        else {
            panic!("expected Name, got {:?}", ty.kind);
        };
        assert_eq!(name_span, crate::Span::new(F, 7, 10));
        assert_eq!(ty.span, crate::Span::new(F, 7, 18));
        assert_eq!(args.len(), 1);
        assert!(matches!(
            args[0].kind,
            crate::TypeAnnotationKind::Name { .. }
        ));
    }

    #[test]
    fn parse_generic_two_args() {
        let (ast, diags) = parse_str("let m: Map<string, number> = null;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let ty = type_of_let(single_stmt(&ast));
        let crate::TypeAnnotationKind::Name { ref args, .. } = ty.kind else {
            panic!("expected Name");
        };
        assert_eq!(args.len(), 2);
    }

    #[test]
    fn parse_generic_nested() {
        let (ast, diags) = parse_str("let b: Box<Box<T>> = null;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let ty = type_of_let(single_stmt(&ast));
        let crate::TypeAnnotationKind::Name { ref args, .. } = ty.kind else {
            panic!("expected outer Name");
        };
        assert_eq!(args.len(), 1);
        let crate::TypeAnnotationKind::Name {
            args: ref inner, ..
        } = args[0].kind
        else {
            panic!("expected inner Name");
        };
        assert_eq!(inner.len(), 1);
    }

    #[test]
    fn parse_generic_array_postfix() {
        let (ast, diags) = parse_str("let xs: Foo<number>[] = null;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let ty = type_of_let(single_stmt(&ast));
        let crate::TypeAnnotationKind::Array(ref inner) = ty.kind else {
            panic!("expected Array");
        };
        assert!(
            matches!(inner.kind, crate::TypeAnnotationKind::Name { ref args, .. } if args.len() == 1)
        );
    }

    #[test]
    fn parse_generic_empty_diagnoses() {
        let (_ast, diags) = parse_str("let x: Foo<> = null;");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("empty generic argument list")),
            "expected empty-generic-args diagnostic, got: {diags:?}"
        );
    }

    #[test]
    fn parse_generic_trailing_comma_ok() {
        let (_ast, diags) = parse_str("let x: Foo<number,> = null;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    #[test]
    fn parse_generic_missing_close_diagnoses() {
        let (_ast, diags) = parse_str("let x: Foo<number = null;");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("expected `,` or `>`")),
            "expected unterminated-generic diagnostic, got: {diags:?}"
        );
    }

    #[test]
    fn parse_qualified_two_segments() {
        let (ast, diags) = parse_str("let x: Foo.Bar = null;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let ty = type_of_let(single_stmt(&ast));
        let crate::TypeAnnotationKind::Qualified { ref path, ref args } = ty.kind else {
            panic!("expected Qualified, got {:?}", ty.kind);
        };
        assert_eq!(path.len(), 2);
        // `Foo` at 7..10, `Bar` at 11..14.
        assert_eq!(path[0], crate::Span::new(F, 7, 10));
        assert_eq!(path[1], crate::Span::new(F, 11, 14));
        assert!(args.is_empty());
        assert_eq!(ty.span, crate::Span::new(F, 7, 14));
    }

    #[test]
    fn parse_qualified_three_segments() {
        let (ast, diags) = parse_str("let x: Foo.Bar.Baz = null;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let ty = type_of_let(single_stmt(&ast));
        let crate::TypeAnnotationKind::Qualified { ref path, .. } = ty.kind else {
            panic!("expected Qualified, got {:?}", ty.kind);
        };
        assert_eq!(path.len(), 3);
    }

    #[test]
    fn parse_qualified_with_generic_args() {
        let (ast, diags) = parse_str("let x: Foo.Box<number> = null;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let ty = type_of_let(single_stmt(&ast));
        let crate::TypeAnnotationKind::Qualified { ref path, ref args } = ty.kind else {
            panic!("expected Qualified, got {:?}", ty.kind);
        };
        assert_eq!(path.len(), 2);
        assert_eq!(args.len(), 1);
        assert!(matches!(
            args[0].kind,
            crate::TypeAnnotationKind::Name { .. }
        ));
    }

    #[test]
    fn parse_qualified_with_array_postfix() {
        let (ast, diags) = parse_str("let xs: Foo.Bar[] = null;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let ty = type_of_let(single_stmt(&ast));
        let crate::TypeAnnotationKind::Array(ref inner) = ty.kind else {
            panic!("expected Array, got {:?}", ty.kind);
        };
        assert!(matches!(
            inner.kind,
            crate::TypeAnnotationKind::Qualified { .. }
        ));
    }

    #[test]
    fn parse_qualified_trailing_dot_diagnoses() {
        let (_ast, diags) = parse_str("let x: Foo. = null;");
        assert!(
            diags.iter().any(|d| d
                .message
                .contains("expected identifier after `.` in type name")),
            "expected dotted-name diagnostic, got: {diags:?}"
        );
    }

    #[test]
    fn parse_qualified_void_root_rejected_dot() {
        let (ast, diags) = parse_str("function f(): void.Bar { }");
        assert!(!diags.is_empty(), "expected a diagnostic for `void.Bar`");
        let _ = ast;
    }

    fn union_members(ty: &crate::TypeAnnotation) -> &[crate::TypeAnnotation] {
        match &ty.kind {
            crate::TypeAnnotationKind::Union(members) => members,
            other => panic!("expected Union, got {other:?}"),
        }
    }

    #[test]
    fn parse_union_two_members() {
        let (ast, diags) = parse_str("let x: number | string = null;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let ty = type_of_let(single_stmt(&ast));
        let members = union_members(ty);
        assert_eq!(members.len(), 2);
        assert!(matches!(
            members[0].kind,
            crate::TypeAnnotationKind::Name { .. }
        ));
        assert_eq!(members[0].span, crate::Span::new(F, 7, 13)); // `number`
        assert!(matches!(
            members[1].kind,
            crate::TypeAnnotationKind::Name { .. }
        ));
        assert_eq!(members[1].span, crate::Span::new(F, 16, 22)); // `string`
        assert_eq!(ty.span, crate::Span::new(F, 7, 22));
    }

    #[test]
    fn parse_union_three_members() {
        let (ast, diags) = parse_str("let x: A | B | C = null;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let ty = type_of_let(single_stmt(&ast));
        let members = union_members(ty);
        assert_eq!(members.len(), 3);
        for m in members {
            assert!(matches!(m.kind, crate::TypeAnnotationKind::Name { .. }));
        }
    }

    #[test]
    fn parse_union_lower_than_array() {
        let (ast, diags) = parse_str("let x: number[] | string = null;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let ty = type_of_let(single_stmt(&ast));
        let members = union_members(ty);
        assert_eq!(members.len(), 2);
        assert!(
            matches!(members[0].kind, crate::TypeAnnotationKind::Array(_)),
            "expected first member to be Array, got {:?}",
            members[0].kind
        );
        assert!(matches!(
            members[1].kind,
            crate::TypeAnnotationKind::Name { .. }
        ));
    }

    #[test]
    fn parse_union_with_null_member() {
        let (ast, diags) = parse_str("let x: number | null = null;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let ty = type_of_let(single_stmt(&ast));
        let members = union_members(ty);
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].span, crate::Span::new(F, 7, 13)); // `number`
        assert!(matches!(
            members[1].kind,
            crate::TypeAnnotationKind::Name { .. }
        ));
        assert_eq!(members[1].span, crate::Span::new(F, 16, 20)); // `null`
    }

    #[test]
    fn parse_union_in_function_return_type() {
        let (ast, diags) = parse_str("function f(): number | string { }");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmt = single_stmt(&ast);
        match stmt.kind {
            crate::StmtKind::Function {
                ref return_type, ..
            } => {
                let return_type = return_type.as_ref().expect("plain return type");
                let members = union_members(return_type);
                assert_eq!(members.len(), 2);
            }
            _ => panic!("expected Function"),
        }
    }

    #[test]
    fn parse_union_in_function_param_type() {
        let (ast, diags) = parse_str("function f(x: number | string): void { }");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmt = single_stmt(&ast);
        match stmt.kind {
            crate::StmtKind::Function { ref params, .. } => {
                let p_ty = params[0].ty.as_ref().expect("param type");
                let members = union_members(p_ty);
                assert_eq!(members.len(), 2);
            }
            _ => panic!("expected Function"),
        }
    }

    #[test]
    fn parse_union_in_object_field_type() {
        let (ast, diags) = parse_str("let p: { x: number | string } = null;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let ty = type_of_let(single_stmt(&ast));
        let crate::TypeAnnotationKind::Object { ref fields } = ty.kind else {
            panic!("expected Object, got {:?}", ty.kind);
        };
        assert_eq!(fields.len(), 1);
        let members = union_members(&fields[0].ty);
        assert_eq!(members.len(), 2);
    }

    #[test]
    fn parse_union_leading_pipe_is_forgiven() {
        let (ast, diags) = parse_str("let x: | number | string = null;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let ty = type_of_let(single_stmt(&ast));
        let members = union_members(ty);
        assert_eq!(members.len(), 2);
    }

    #[test]
    fn parse_union_leading_pipe_single_member_unwraps() {
        let (ast, diags) = parse_str("let x: | number = null;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let ty = type_of_let(single_stmt(&ast));
        assert!(matches!(ty.kind, crate::TypeAnnotationKind::Name { .. }));
    }

    #[test]
    fn parse_union_trailing_pipe_diagnoses() {
        let (_, diags) = parse_str("let x: number | = null;");
        assert!(!diags.is_empty());
        assert_eq!(diags[0].message, "expected type");
    }

    #[test]
    fn snapshot_union_type_mix() {
        let (ast, diags) = parse_str("let xs: number[] | string | null = null;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        insta::assert_debug_snapshot!(ast);
    }

    #[test]
    fn parse_object_type_duplicate_field_diagnoses() {
        let (_, diags) = parse_str("let p: { readonly x: number; x: string } = null;");
        assert!(!diags.is_empty());
        assert!(
            diags
                .iter()
                .any(|d| d.message == "duplicate field `x` in object type"),
            "expected duplicate-field diagnostic, got: {diags:?}"
        );
    }

    #[test]
    fn snapshot_object_and_array_literals() {
        let (ast, diags) = parse_str(r#"let mix = [{ a: 1, b: "hi" }, {}];"#);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        insta::assert_debug_snapshot!(ast);
    }

    #[test]
    fn snapshot_function_with_while() {
        let source = "function loop(n: number): void {\n  while (n > 0) {\n    log(n);\n  }\n}";
        let (ast, diags) = parse_str(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        insta::assert_debug_snapshot!(ast);
    }

    fn parse_arrow_const(
        source: &str,
    ) -> (
        Ast,
        Vec<crate::ParamDecl>,
        Option<crate::TypeAnnotation>,
        crate::ArrowBody,
    ) {
        let (ast, diags) = parse_str(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(ast.top_level.len(), 1);
        let stmt = ast.stmt(ast.top_level[0]);
        let value = match &stmt.kind {
            StmtKind::Const { value, .. } => *value,
            _ => panic!("expected const declaration"),
        };
        let expr = ast.expr(value).clone();
        let (params, return_type, body) = match expr.kind {
            ExprKind::Arrow {
                params,
                return_type,
                body,
                ..
            } => (params, return_type, body),
            other => panic!("expected Arrow, got {other:?}"),
        };
        (ast, params, return_type, body)
    }

    #[test]
    fn parses_anonymous_function_expression_as_arrow() {
        let (ast, params, return_type, body) =
            parse_arrow_const("const f = function (x: number): number { return x + 1; };");
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name.name, "x");
        assert!(return_type.is_some());
        assert!(matches!(body, crate::ArrowBody::Block(_)));
        let _ = ast;
    }

    #[test]
    fn parses_function_expression_with_no_params() {
        let (_ast, params, _return_type, body) =
            parse_arrow_const("const f = function (): void {};");
        assert!(params.is_empty());
        assert!(matches!(body, crate::ArrowBody::Block(_)));
    }

    /// The name is a label with no binding of its own, so it is dropped.
    #[test]
    fn parses_named_function_expression_by_dropping_the_name() {
        let (_ast, params, _return_type, _body) =
            parse_arrow_const("const f = function named(x: number): number { return x + 1; };");
        assert_eq!(params.len(), 1);
    }

    /// Dropping the name would change meaning here, so this one is rejected.
    #[test]
    fn rejects_a_named_function_expression_that_calls_itself() {
        let (_ast, diags) =
            parse_str("const f = function bar(x: number): number { return bar(x); };");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("cannot call itself")),
            "self-referential named function expression should be rejected: {diags:?}"
        );
    }

    /// A member name after `.` is never a reference to the function itself.
    #[test]
    fn accepts_a_named_function_expression_using_the_name_as_a_property() {
        let (_ast, diags) = parse_str("const f = function bar(o: O): number { return o.bar; };");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    /// Nor is a key before `:`.
    #[test]
    fn accepts_a_named_function_expression_using_the_name_as_an_object_key() {
        let (_ast, diags) = parse_str(
            "const f = function bar(x: number): number { const o = { bar: 1 }; return o.bar + x; };",
        );
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    /// A string that happens to spell the name is not a reference.
    #[test]
    fn accepts_a_named_function_expression_with_its_name_in_a_string() {
        let (_ast, diags) =
            parse_str("const f = function bar(x: number): string { return \"bar\"; };");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    /// A template substitution holds real identifier tokens, so a call there is caught.
    #[test]
    fn rejects_a_self_call_inside_a_template_substitution() {
        let (_ast, diags) =
            parse_str("const f = function bar(x: number): string { return `${bar(0)}`; };");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("cannot call itself")),
            "a self-call in a template substitution should be rejected: {diags:?}"
        );
    }

    #[test]
    fn rejects_a_generic_function_expression() {
        let (_ast, diags) = parse_str("const f = function <T>(x: T): T { return x; };");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("generic function expressions")),
            "generic function expression should be rejected: {diags:?}"
        );
    }

    /// Both forms lower to an arrow, which captures `this` lexically, while TypeScript
    /// rebinds it per call. Inside a class method those disagree, so `this` is rejected
    /// there rather than silently resolving to the enclosing instance.
    #[test]
    fn rejects_this_inside_a_function_expression_in_a_class_method() {
        let (_ast, diags) = parse_str(
            "class C { x: number = 1; m(): number { \
             const f = function (): number { return this.x; }; return f(); } }",
        );
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("`this` is only valid inside")),
            "`this` in a function expression should be rejected: {diags:?}"
        );
    }

    #[test]
    fn rejects_this_inside_method_shorthand_in_a_class_method() {
        let (_ast, diags) = parse_str(
            "class C { x: number = 1; m(): number { \
             const o = { x: 2, g(): number { return this.x; } }; return o.g(); } }",
        );
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("`this` is only valid inside")),
            "`this` in method shorthand should be rejected: {diags:?}"
        );
    }

    #[test]
    fn this_still_parses_in_an_ordinary_class_method() {
        let (_ast, diags) = parse_str("class C { x: number = 1; m(): number { return this.x; } }");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    /// `function` is a valid property name; the expression form must not shadow that.
    #[test]
    fn function_keyword_still_parses_as_an_object_key() {
        let (_ast, diags) = parse_str("let o = { function: 1 };");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    fn object_literal_member_values(source: &str) -> (Ast, Vec<crate::ExprId>) {
        let (ast, diags) = parse_str(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmt = ast.stmt(ast.top_level[0]);
        let value = match &stmt.kind {
            StmtKind::Const { value, .. } => *value,
            other => panic!("expected a const declaration, got {other:?}"),
        };
        let members = match &ast.expr(value).kind {
            ExprKind::ObjectLiteral { members } => members
                .iter()
                .map(crate::ast::ObjectLiteralMember::value)
                .collect::<Vec<_>>(),
            other => panic!("expected ObjectLiteral, got {other:?}"),
        };
        (ast, members)
    }

    #[test]
    fn parses_object_literal_method_shorthand_as_arrow() {
        let (ast, members) =
            object_literal_member_values("const o = { m(x: number): number { return x; } };");
        assert_eq!(members.len(), 1);
        match &ast.expr(members[0]).kind {
            ExprKind::Arrow { params, body, .. } => {
                assert_eq!(params.len(), 1);
                assert_eq!(params[0].name.name, "x");
                assert!(matches!(body, crate::ArrowBody::Block(_)));
            }
            other => panic!("expected Arrow, got {other:?}"),
        }
    }

    /// A keyword is a valid method name, just as it is a valid property name.
    #[test]
    fn parses_method_shorthand_with_a_keyword_name() {
        let (ast, members) =
            object_literal_member_values("const o = { if(x: number): number { return x; } };");
        assert!(matches!(ast.expr(members[0]).kind, ExprKind::Arrow { .. }));
    }

    #[test]
    fn parses_method_shorthand_alongside_plain_and_shorthand_properties() {
        let (ast, members) =
            object_literal_member_values("const o = { a: 1, m(): void {}, b: 2 };");
        assert_eq!(members.len(), 3);
        assert!(matches!(ast.expr(members[1]).kind, ExprKind::Arrow { .. }));
    }

    #[test]
    fn parses_bare_ident_arrow_expression_body() {
        let (_ast, params, return_type, body) = parse_arrow_const("const f = x => x + 1;");
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name.name, "x");
        assert!(params[0].ty.is_none());
        assert!(return_type.is_none());
        assert!(matches!(body, crate::ArrowBody::Expr(_)));
    }

    #[test]
    fn parses_zero_param_arrow() {
        let (_ast, params, return_type, body) = parse_arrow_const("const f = () => 0;");
        assert!(params.is_empty());
        assert!(return_type.is_none());
        assert!(matches!(body, crate::ArrowBody::Expr(_)));
    }

    #[test]
    fn parses_paren_unannotated_arrow() {
        let (_ast, params, _return_type, _body) = parse_arrow_const("const f = (x) => x;");
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name.name, "x");
        assert!(params[0].ty.is_none());
    }

    #[test]
    fn parses_typed_param_arrow() {
        let (_ast, params, return_type, body) =
            parse_arrow_const("const f = (x: number) => x * 2;");
        assert_eq!(params.len(), 1);
        assert!(params[0].ty.is_some());
        assert!(return_type.is_none());
        assert!(matches!(body, crate::ArrowBody::Expr(_)));
    }

    #[test]
    fn parses_typed_param_and_return_arrow() {
        let (_ast, params, return_type, body) =
            parse_arrow_const("const f = (x: number): number => x * 2;");
        assert_eq!(params.len(), 1);
        assert!(params[0].ty.is_some());
        assert!(return_type.is_some());
        assert!(matches!(body, crate::ArrowBody::Expr(_)));
    }

    #[test]
    fn parses_multi_param_unannotated_arrow() {
        let (_ast, params, _return_type, _body) = parse_arrow_const("const f = (x, y) => x + y;");
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name.name, "x");
        assert_eq!(params[1].name.name, "y");
        assert!(params[0].ty.is_none());
        assert!(params[1].ty.is_none());
    }

    #[test]
    fn parses_multi_param_typed_arrow() {
        let (_ast, params, _return_type, _body) =
            parse_arrow_const("const f = (x: number, y: number) => x + y;");
        assert_eq!(params.len(), 2);
        assert!(params[0].ty.is_some());
        assert!(params[1].ty.is_some());
    }

    #[test]
    fn parses_block_body_arrow() {
        let (_ast, params, _return_type, body) =
            parse_arrow_const("const f = (x: number) => { return x; };");
        assert_eq!(params.len(), 1);
        assert!(matches!(body, crate::ArrowBody::Block(_)));
    }

    #[test]
    fn rejects_typed_bare_ident_arrow() {
        let (_ast, diags) = parse_str("const f = x: number => x;");
        assert!(
            !diags.is_empty(),
            "expected diagnostics for `x: number => …`",
        );
    }

    #[test]
    fn arrow_inside_call_argument_parses() {
        let (ast, diags) = parse_str("f((x: number) => x + 1, 2);");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmt = ast.stmt(ast.top_level[0]);
        let call = match &stmt.kind {
            StmtKind::Expr(eid) => ast.expr(*eid),
            _ => panic!("expected expression statement"),
        };
        let args = match &call.kind {
            ExprKind::Call { args, .. } => args.clone(),
            other => panic!("expected Call, got {other:?}"),
        };
        assert_eq!(args.len(), 2);
        assert!(matches!(ast.expr(args[0]).kind, ExprKind::Arrow { .. }));
    }

    #[test]
    fn function_decl_param_still_requires_annotation() {
        let (_ast, diags) = parse_str("function f(x): number { return x; }");
        assert!(
            diags.iter().any(|d| d.message.contains("type annotation")),
            "expected `parameter requires a type annotation` diagnostic, got {diags:?}",
        );
    }

    #[test]
    fn paren_expression_still_parses_when_no_arrow() {
        let (ast, diags) = parse_str("const a = (1 + 2);");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let stmt = ast.stmt(ast.top_level[0]);
        let value = match &stmt.kind {
            StmtKind::Const { value, .. } => *value,
            _ => panic!("expected const declaration"),
        };
        assert!(matches!(ast.expr(value).kind, ExprKind::Paren(_)));
    }

    #[test]
    fn parse_named_import_single() {
        let (ast, diags) = parse_str("import { v4 } from \"submilli:uuid\";");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(ast.top_level.len(), 1);
        let stmt = ast.stmt(ast.top_level[0]);
        let (module, kind) = match &stmt.kind {
            StmtKind::Import { module, kind, .. } => (module, kind),
            other => panic!("expected Import, got {other:?}"),
        };
        assert_eq!(module, "submilli:uuid");
        let specs = match kind {
            ImportKind::Named(specs) => specs,
            other => panic!("expected Named, got {other:?}"),
        };
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].imported_name.name, "v4");
        assert_eq!(specs[0].local_name.name, "v4");
    }

    #[test]
    fn parse_named_import_multi_and_alias() {
        let (ast, diags) =
            parse_str("import { v4, v7 as makeId, validate } from \"submilli:uuid\";");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let specs = match &ast.stmt(ast.top_level[0]).kind {
            StmtKind::Import {
                kind: ImportKind::Named(s),
                ..
            } => s.clone(),
            other => panic!("expected named import, got {other:?}"),
        };
        assert_eq!(specs.len(), 3);
        assert_eq!(specs[0].imported_name.name, "v4");
        assert_eq!(specs[0].local_name.name, "v4");
        assert_eq!(specs[1].imported_name.name, "v7");
        assert_eq!(specs[1].local_name.name, "makeId");
        assert_eq!(specs[2].imported_name.name, "validate");
        assert_eq!(specs[2].local_name.name, "validate");
    }

    #[test]
    fn parse_namespace_import() {
        let (ast, diags) = parse_str("import uuid from \"submilli:uuid\";");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let kind = match &ast.stmt(ast.top_level[0]).kind {
            StmtKind::Import { kind, .. } => kind.clone(),
            other => panic!("expected import, got {other:?}"),
        };
        let local = match kind {
            ImportKind::Namespace { local_name } => local_name,
            other => panic!("expected Namespace, got {other:?}"),
        };
        assert_eq!(local.name, "uuid");
    }

    #[test]
    fn parse_wildcard_namespace_import_synonym() {
        let (ast, diags) = parse_str("import * as uuid from \"submilli:uuid\";");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let kind = match &ast.stmt(ast.top_level[0]).kind {
            StmtKind::Import { kind, .. } => kind.clone(),
            other => panic!("expected import, got {other:?}"),
        };
        let local = match kind {
            ImportKind::Namespace { local_name } => local_name,
            other => panic!("expected Namespace, got {other:?}"),
        };
        assert_eq!(local.name, "uuid");
    }

    #[test]
    fn parse_wildcard_namespace_import_without_as_rejected() {
        let (_ast, diags) = parse_str("import * from \"submilli:uuid\";");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("expected `as <name>`")),
            "expected missing-as diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn parse_combined_default_and_named_rejected() {
        let (_ast, diags) = parse_str("import uuid, { v4 } from \"submilli:uuid\";");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("combining default and named")),
            "expected combined-import diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn parse_side_effect_import_rejected() {
        let (_ast, diags) = parse_str("import \"submilli:uuid\";");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("side-effect-only imports")),
            "expected side-effect-import diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn parse_empty_specifier_list_rejected() {
        let (_ast, diags) = parse_str("import {} from \"submilli:uuid\";");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("empty import specifier list")),
            "expected empty-specifier diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn parse_import_inside_function_body_rejected() {
        let (_ast, diags) =
            parse_str("function main(): void { import { v4 } from \"submilli:uuid\"; }");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("must appear at the top of the file")),
            "expected top-level diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn parse_missing_from_rejected() {
        let (_ast, diags) = parse_str("import { v4 } \"submilli:uuid\";");
        assert!(
            diags.iter().any(|d| d.message.contains("expected `from`")),
            "expected missing-from diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn parse_missing_module_specifier_rejected() {
        let (_ast, diags) = parse_str("import { v4 } from;");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("expected module specifier")),
            "expected missing-module diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn parse_export_function_marks_exported() {
        // `export function main() {}` parses to a single function declaration in
        // `top_level`, plus an `exported_decls` entry pointing at it.
        let (ast, diags) = parse_str("export function main(): void {}");
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        let stmt = single_stmt(&ast);
        assert!(
            matches!(stmt.kind, crate::StmtKind::Function { .. }),
            "expected Function decl, got {:?}",
            stmt.kind,
        );
        assert_eq!(ast.exported_decls.len(), 1, "one exported decl");
        assert_eq!(ast.exported_decls[0].stmt, ast.top_level[0]);
    }

    #[test]
    fn parse_export_const_type_interface_enum_are_marked() {
        for src in [
            "export const X: number = 1;",
            "export type Id = string;",
            "export interface P { x: number; }",
            "export enum E { A, B }",
        ] {
            let (ast, diags) = parse_str(src);
            assert!(diags.is_empty(), "unexpected diags for {src:?}: {diags:?}");
            assert_eq!(ast.top_level.len(), 1, "one decl for {src:?}");
            assert_eq!(ast.exported_decls.len(), 1, "one exported decl for {src:?}");
        }
    }

    #[test]
    fn parse_export_default_is_rejected_with_focused_message() {
        let (_ast, diags) = parse_str("export default function main(): void {}");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("`export default` is not supported")),
            "expected focused export-default diagnostic, got: {diags:?}",
        );
        assert!(
            !diags.iter().any(|d| d.message.contains("expected `;`")),
            "the spurious missing-semicolon error must be gone: {diags:?}",
        );
    }

    #[test]
    fn parse_export_list_parses_as_export_from() {
        // Form 2 (`export { x };`, no `from`) now parses cleanly — the
        // single-file gating happens in the typechecker, not the parser.
        let (ast, diags) = parse_str("function main(): void {}\nexport { main };");
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        assert_eq!(ast.top_level.len(), 2, "function + export-from");
        let export = ast.stmt(ast.top_level[1]);
        let crate::StmtKind::ExportFrom { specs, source, .. } = &export.kind else {
            panic!("expected ExportFrom, got {:?}", export.kind);
        };
        assert!(source.is_none(), "bare `export {{ x }}` has no source");
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].imported_name.name, "main");
        assert_eq!(specs[0].local_name.name, "main");
    }

    #[test]
    fn parse_export_from_with_source_and_alias_parses() {
        let (ast, diags) = parse_str("export { foo, bar as baz } from \"./util\";");
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        let export = ast.stmt(ast.top_level[0]);
        let crate::StmtKind::ExportFrom { specs, source, .. } = &export.kind else {
            panic!("expected ExportFrom, got {:?}", export.kind);
        };
        let Some((module, _)) = source else {
            panic!("expected a `from` source");
        };
        assert_eq!(module, "./util");
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[1].imported_name.name, "bar");
        assert_eq!(specs[1].local_name.name, "baz");
    }

    #[test]
    fn parse_export_inside_function_body_rejected() {
        let (_ast, diags) = parse_str("function main(): void { export const X: number = 1; }");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("must appear at the top of the file")),
            "expected top-level diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn parse_throw_with_expression() {
        let (ast, diags) = parse_str(r#"throw new Error("oops");"#);
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::Throw { value } = stmt.kind else {
            panic!("expected Throw, got {:?}", stmt.kind);
        };
        assert!(matches!(ast.expr(value).kind, crate::ExprKind::New { .. }));
    }

    #[test]
    fn parse_new_with_dotted_callee() {
        let (ast, diags) = parse_str("new Temporal.Duration({});");
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        let crate::StmtKind::Expr(expr_id) = single_stmt(&ast).kind else {
            panic!("expected expression statement");
        };
        let crate::ExprKind::New { callee, .. } = &ast.expr(expr_id).kind else {
            panic!("expected New");
        };
        let crate::ExprKind::FieldAccess { name, .. } = &ast.expr(*callee).kind else {
            panic!("expected FieldAccess callee");
        };
        assert_eq!(name.name, "Duration");
    }

    #[test]
    fn parse_new_with_three_level_dotted_callee() {
        let (ast, diags) = parse_str("new Foo.Bar.Baz();");
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        let crate::StmtKind::Expr(expr_id) = single_stmt(&ast).kind else {
            panic!("expected expression statement");
        };
        let crate::ExprKind::New { callee, .. } = &ast.expr(expr_id).kind else {
            panic!("expected New");
        };
        let crate::ExprKind::FieldAccess { receiver, name } = &ast.expr(*callee).kind else {
            panic!("expected outer FieldAccess");
        };
        assert_eq!(name.name, "Baz");
        let crate::ExprKind::FieldAccess { name: mid_name, .. } = &ast.expr(*receiver).kind else {
            panic!("expected inner FieldAccess");
        };
        assert_eq!(mid_name.name, "Bar");
    }

    #[test]
    fn parse_new_with_trailing_dot_diagnoses() {
        let (_ast, diags) = parse_str("new Foo.;");
        assert!(
            diags.iter().any(|d| d
                .message
                .contains("expected identifier after `.` in constructor name")),
            "expected trailing-dot diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn throw_without_expression_diagnoses() {
        let (_ast, diags) = parse_str("throw;");
        assert!(
            diags
                .iter()
                .any(|d| d.message == "expected expression after `throw`"),
            "expected bare-throw diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn parse_try_catch() {
        let (ast, diags) = parse_str("try { } catch (e: Error) { }");
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::Try {
            body,
            ref catches,
            ref finally,
        } = stmt.kind
        else {
            panic!("expected Try, got {:?}", stmt.kind);
        };
        assert!(matches!(
            ast.stmt(body).kind,
            crate::StmtKind::Block(ref v) if v.is_empty()
        ));
        let [clause] = catches.as_slice() else {
            panic!("expected one catch clause, got {catches:?}");
        };
        assert_eq!(clause.binding.name, "e");
        assert!(clause.ty.is_some());
        assert!(finally.is_none());
    }

    #[test]
    fn parse_try_finally() {
        let (ast, diags) = parse_str("try { } finally { }");
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::Try {
            ref catches,
            ref finally,
            ..
        } = stmt.kind
        else {
            panic!("expected Try");
        };
        assert!(catches.is_empty());
        let finally_id = finally.expect("finally block");
        assert!(matches!(
            ast.stmt(finally_id).kind,
            crate::StmtKind::Block(_)
        ));
    }

    #[test]
    fn parse_try_catch_finally() {
        let (ast, diags) = parse_str("try { } catch (e: Error) { } finally { }");
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::Try {
            ref catches,
            ref finally,
            ..
        } = stmt.kind
        else {
            panic!("expected Try");
        };
        assert_eq!(catches.len(), 1);
        assert!(finally.is_some());
    }

    #[test]
    fn bare_try_diagnoses() {
        let (_ast, diags) = parse_str("try { }");
        assert!(
            diags
                .iter()
                .any(|d| d.message == "try requires at least one of `catch` or `finally`"),
            "expected bare-try diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn parse_untyped_catch() {
        let (ast, diags) = parse_str("try { } catch (e) { }");
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::Try {
            ref catches,
            ref finally,
            ..
        } = stmt.kind
        else {
            panic!("expected Try");
        };
        let [clause] = catches.as_slice() else {
            panic!("expected one catch clause, got {catches:?}");
        };
        assert_eq!(clause.binding.name, "e");
        assert!(clause.ty.is_none());
        assert!(finally.is_none());
    }

    #[test]
    fn parse_multiple_catch_clauses() {
        let (ast, diags) = parse_str("try { } catch (e: HttpError) { } catch (f) { }");
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        let stmt = single_stmt(&ast);
        let crate::StmtKind::Try {
            ref catches,
            ref finally,
            ..
        } = stmt.kind
        else {
            panic!("expected Try");
        };
        let [first, second] = catches.as_slice() else {
            panic!("expected two catch clauses, got {catches:?}");
        };
        assert_eq!(first.binding.name, "e");
        assert!(first.ty.is_some());
        assert_eq!(second.binding.name, "f");
        assert!(second.ty.is_none());
        assert!(finally.is_none());
    }

    #[test]
    fn catch_after_finally_diagnoses() {
        let (_ast, diags) = parse_str("try { } finally { } catch (e: Error) { }");
        assert!(
            !diags.is_empty(),
            "expected diagnostic for catch-after-finally"
        );
    }

    #[test]
    fn catch_missing_open_paren_diagnoses() {
        let (_ast, diags) = parse_str("try { } catch e: Error) { }");
        assert!(
            diags
                .iter()
                .any(|d| d.message == "expected `(` after `catch`"),
            "expected open-paren diagnostic, got: {diags:?}",
        );
    }

    fn class_members(ast: &Ast) -> &[crate::ClassMember] {
        match &single_stmt(ast).kind {
            StmtKind::ClassDecl { members, .. } => members,
            other => panic!("expected ClassDecl, got {other:?}"),
        }
    }

    #[test]
    fn parse_animal_class() {
        let src = r#"
class Animal {
  name: string;
  private sound: string;
  readonly species: string;

  constructor(name: string, sound: string, species: string) {
    this.name = name;
    this.sound = sound;
    this.species = species;
  }

  speak(): string {
    return this.name + " says " + this.sound;
  }
}
"#;
        let (ast, diags) = parse_str(src);
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        let StmtKind::ClassDecl {
            name,
            generics,
            extends,
            implements,
            members,
            ..
        } = &single_stmt(&ast).kind
        else {
            panic!("expected ClassDecl");
        };
        assert_eq!(name.name, "Animal");
        assert!(generics.is_empty());
        assert!(extends.is_none());
        assert!(implements.is_empty());
        assert_eq!(members.len(), 5);

        let crate::ClassMember::Field {
            name, modifiers, ..
        } = &members[0]
        else {
            panic!("member 0 should be a field");
        };
        assert_eq!(name.name, "name");
        assert_eq!(modifiers.visibility, crate::Visibility::Public);
        assert!(modifiers.readonly.is_none());

        let crate::ClassMember::Field { modifiers, .. } = &members[1] else {
            panic!("member 1 should be a field");
        };
        assert_eq!(modifiers.visibility, crate::Visibility::Private);

        let crate::ClassMember::Field { modifiers, .. } = &members[2] else {
            panic!("member 2 should be a field");
        };
        assert!(modifiers.readonly.is_some());
        assert_eq!(modifiers.visibility, crate::Visibility::Public);

        assert!(matches!(
            &members[3],
            crate::ClassMember::Constructor { .. }
        ));

        let crate::ClassMember::Method { name, .. } = &members[4] else {
            panic!("member 4 should be a method");
        };
        assert_eq!(name.name, "speak");
    }

    #[test]
    fn parse_dog_class_extends_super_and_void_method() {
        let src = r#"
class Dog extends Animal {
  private tricks: string[];

  constructor(name: string) {
    super(name, "woof", "canis familiaris");
    this.tricks = [];
  }

  learn(trick: string): void {
    this.tricks.push(trick);
  }
}
"#;
        let (ast, diags) = parse_str(src);
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        let StmtKind::ClassDecl {
            extends, members, ..
        } = &single_stmt(&ast).kind
        else {
            panic!("expected ClassDecl");
        };
        assert!(extends.is_some(), "Dog should extend Animal");
        assert_eq!(members.len(), 3);
        assert!(matches!(&members[0], crate::ClassMember::Field { .. }));
        assert!(matches!(
            &members[1],
            crate::ClassMember::Constructor { .. }
        ));
        assert!(matches!(&members[2], crate::ClassMember::Method { .. }));
    }

    #[test]
    fn parse_class_implements_list() {
        let (ast, diags) = parse_str("class C implements I, J { run(): void {} }");
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        let StmtKind::ClassDecl { implements, .. } = &single_stmt(&ast).kind else {
            panic!("expected ClassDecl");
        };
        assert_eq!(implements.len(), 2);
    }

    #[test]
    fn parse_class_extends_and_implements() {
        let (ast, diags) = parse_str("class C extends B implements I { run(): void {} }");
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        let StmtKind::ClassDecl {
            extends,
            implements,
            ..
        } = &single_stmt(&ast).kind
        else {
            panic!("expected ClassDecl");
        };
        assert!(extends.is_some());
        assert_eq!(implements.len(), 1);
    }

    #[test]
    fn parse_class_generics_optional_field_and_initializer() {
        let (ast, diags) = parse_str("class Box<T> { value?: T; items: T[] = []; }");
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        let StmtKind::ClassDecl {
            generics, members, ..
        } = &single_stmt(&ast).kind
        else {
            panic!("expected ClassDecl");
        };
        assert_eq!(generics.len(), 1);
        assert_eq!(generics[0].name, "T");

        let crate::ClassMember::Field {
            optional,
            initializer,
            ..
        } = &members[0]
        else {
            panic!("member 0 should be a field");
        };
        assert!(*optional);
        assert!(initializer.is_none());

        let crate::ClassMember::Field { initializer, .. } = &members[1] else {
            panic!("member 1 should be a field");
        };
        assert!(initializer.is_some());
    }

    #[test]
    fn export_class_is_marked() {
        let (ast, diags) = parse_str("export class Foo {}");
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        assert_eq!(ast.top_level.len(), 1);
        assert_eq!(ast.exported_decls.len(), 1);
        assert!(matches!(
            &single_stmt(&ast).kind,
            StmtKind::ClassDecl { .. }
        ));
    }

    #[test]
    fn conflicting_visibility_modifiers_diagnose() {
        let (_ast, diags) = parse_str("class C { public private x: number; }");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("at most one visibility modifier")),
            "got: {diags:?}",
        );
    }

    #[test]
    fn duplicate_readonly_modifier_diagnoses() {
        let (_ast, diags) = parse_str("class C { readonly readonly x: number; }");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("duplicate `readonly`")),
            "got: {diags:?}",
        );
    }

    #[test]
    fn field_named_like_a_modifier_is_allowed() {
        let (ast, diags) = parse_str("class C { private: number; }");
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        let crate::ClassMember::Field {
            name, modifiers, ..
        } = &class_members(&ast)[0]
        else {
            panic!("expected a field");
        };
        assert_eq!(name.name, "private");
        assert_eq!(modifiers.visibility, crate::Visibility::Public);
    }

    #[test]
    fn method_named_get_is_allowed() {
        let (ast, diags) = parse_str("class C { get(): number { return 1; } }");
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        let crate::ClassMember::Method { name, .. } = &class_members(&ast)[0] else {
            panic!("expected a method");
        };
        assert_eq!(name.name, "get");
    }

    #[test]
    fn static_method_parses() {
        let (ast, diags) = parse_str("class C { static f(): number { return 1; } }");
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        let crate::ClassMember::Method {
            name, modifiers, ..
        } = &class_members(&ast)[0]
        else {
            panic!("expected a method");
        };
        assert_eq!(name.name, "f");
        assert!(modifiers.static_span.is_some());
    }

    #[test]
    fn static_readonly_field_parses() {
        let (ast, diags) = parse_str("class C { static readonly x: number = 1; }");
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        let crate::ClassMember::Field {
            name,
            modifiers,
            initializer,
            ..
        } = &class_members(&ast)[0]
        else {
            panic!("expected a field");
        };
        assert_eq!(name.name, "x");
        assert!(modifiers.static_span.is_some());
        assert!(modifiers.readonly.is_some());
        assert!(initializer.is_some());
    }

    #[test]
    fn private_static_method_parses() {
        let (ast, diags) = parse_str("class C { private static f(): void {} }");
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        let crate::ClassMember::Method { modifiers, .. } = &class_members(&ast)[0] else {
            panic!("expected a method");
        };
        assert_eq!(modifiers.visibility, crate::Visibility::Private);
        assert!(modifiers.static_span.is_some());
    }

    #[test]
    fn method_named_static_is_allowed() {
        let (ast, diags) = parse_str("class C { static(): number { return 1; } }");
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        let crate::ClassMember::Method {
            name, modifiers, ..
        } = &class_members(&ast)[0]
        else {
            panic!("expected a method");
        };
        assert_eq!(name.name, "static");
        assert!(modifiers.static_span.is_none());
    }

    #[test]
    fn visibility_after_static_diagnoses() {
        let (_ast, diags) = parse_str("class C { static private f(): void {} }");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("`private` must come before `static`")),
            "got: {diags:?}",
        );
    }

    #[test]
    fn duplicate_static_modifier_diagnoses() {
        let (_ast, diags) = parse_str("class C { static static f(): void {} }");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("duplicate `static` modifier")),
            "got: {diags:?}",
        );
    }

    #[test]
    fn static_accessor_is_rejected() {
        let (_ast, diags) = parse_str("class C { static get x(): number { return 1; } }");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("static accessors are not supported")),
            "got: {diags:?}",
        );
    }

    #[test]
    fn static_constructor_is_rejected() {
        let (_ast, diags) = parse_str("class C { static constructor() {} }");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("a constructor cannot be `static`")),
            "got: {diags:?}",
        );
    }

    #[test]
    fn optional_static_field_is_rejected() {
        let (_ast, diags) = parse_str("class C { static x?: number; }");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("a static field cannot be optional")),
            "got: {diags:?}",
        );
    }

    #[test]
    fn protected_member_is_rejected() {
        let (_ast, diags) = parse_str("class C { protected x: number; }");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("`protected` is not supported")),
            "got: {diags:?}",
        );
    }

    #[test]
    fn abstract_member_is_rejected() {
        let (_ast, diags) = parse_str("class C { abstract run(): void; }");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("abstract classes are not supported")),
            "got: {diags:?}",
        );
    }

    #[test]
    fn getter_parses_as_accessor() {
        let (ast, diags) = parse_str("class C { get value(): number { return 1; } }");
        assert!(diags.is_empty(), "got: {diags:?}");
        let crate::ClassMember::Accessor { kind, name, .. } = &class_members(&ast)[0] else {
            panic!("expected an accessor member");
        };
        assert_eq!(*kind, crate::AccessorKind::Get);
        assert_eq!(name.name, "value");
    }

    #[test]
    fn setter_parses_as_accessor() {
        let (ast, diags) = parse_str("class C { set value(v: number) {} }");
        assert!(diags.is_empty(), "got: {diags:?}");
        let crate::ClassMember::Accessor { kind, param, .. } = &class_members(&ast)[0] else {
            panic!("expected an accessor member");
        };
        assert_eq!(*kind, crate::AccessorKind::Set);
        assert!(param.is_some(), "setter should have a parameter");
    }

    #[test]
    fn param_property_modifier_outside_constructor_is_rejected() {
        let (_ast, diags) = parse_str("function f(public x: number): void {}");
        assert!(
            diags.iter().any(|d| d
                .message
                .contains("parameter properties are only allowed in a constructor")),
            "got: {diags:?}",
        );
    }

    #[test]
    fn constructor_return_type_is_rejected() {
        let (_ast, diags) = parse_str("class C { constructor(): void {} }");
        assert!(
            diags.iter().any(|d| d
                .message
                .contains("constructor cannot declare a return type")),
            "got: {diags:?}",
        );
    }

    #[test]
    fn field_initializer_without_type_is_rejected() {
        let (_ast, diags) = parse_str("class C { x = 1; }");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("class fields require a type annotation")),
            "got: {diags:?}",
        );
    }

    #[test]
    fn this_outside_method_is_rejected() {
        let (_ast, diags) = parse_str("function f(): string { return this.name; }");
        assert!(
            diags.iter().any(|d| d
                .message
                .contains("`this` is only valid inside a class method or constructor body")),
            "got: {diags:?}",
        );
    }

    #[test]
    fn super_outside_method_is_rejected() {
        let (_ast, diags) = parse_str("function f(): void { super(1); }");
        assert!(
            diags.iter().any(|d| d
                .message
                .contains("`super` is only valid inside a class method or constructor body")),
            "got: {diags:?}",
        );
    }

    #[test]
    fn this_inside_method_is_accepted_by_parser() {
        let (_ast, diags) = parse_str("class C { x: number; read(): number { return this.x; } }");
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
    }
}
