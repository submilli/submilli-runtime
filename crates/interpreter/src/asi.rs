use crate::{Diagnostic, FileId, Lexer, Span, Token, TokenKind};

/// Statement blocks need a terminator before `}`. Value lists and type member
/// lists do not; member lists still need separators between their entries.
#[derive(Clone, Copy, PartialEq)]
enum BraceKind {
    Block,
    Value,
    /// Type members need separators even inside parentheses, but none before `}`.
    MemberList,
}

/// An open `{` and the `(`/`[`/`${` nesting depth it was opened at. A statement block
/// re-enters statement context even when it sits inside parentheses — the body of
/// `arr.map(x => {` ⏎ `…` ⏎ `})` is statements, not a paren-wrapped expression — so
/// "are we inside brackets?" is measured against the enclosing block's base, not zero.
/// A value list carries its enclosing base forward. Member lists reset it so type
/// members receive separators even inside a parameter list.
struct BraceFrame {
    kind: BraceKind,
    bracket_base: u32,
}

/// Automatic Semicolon Insertion: strips `Newline` tokens, inserts `Semicolon` at statement boundaries.
pub struct Asi<'a> {
    lexer: Lexer<'a>,
    buffered: Option<Token>,
    lookahead: std::collections::VecDeque<Token>,
    last_emitted: Option<TokenKind>,
    /// Open `(`, `[`, and template substitutions (`${`), which all suspend ASI: a
    /// newline inside one is a wrapped expression, never a statement end.
    bracket_depth: u32,
    braces: Vec<BraceFrame>,
    // Tracks, per open `(`, whether it is a control-flow header (`if (…)`, `while (…)`,
    // `for (…)`, `switch (…)`, `catch (…)`). The `)` closing such a header is followed by a
    // body, not a statement end, so ASI must not insert a `;` after it.
    paren_is_header: Vec<bool>,
    closed_header_paren: bool,
    closed_block_brace: bool,
    /// `<`/`>` nesting inside the type of a `<T>expr` cast, or 0 outside one. A cast's
    /// `<` stands where an operand is expected; every other `<` follows an operand (a
    /// comparison) or a name (`Array<`, `f<`, `class Box<`), so the position alone
    /// tells them apart.
    cast_angle_depth: u32,
    /// Did `last_emitted` close a cast's type? The `{` after it is the cast's operand —
    /// an object literal — where after any other `>` it opens a body
    /// (`function f(): Array<number> {`).
    closed_cast_angle: bool,
    /// Set between a `class` / `interface` / `enum` keyword and the `{` opening its body.
    /// A declaration header is not a statement, so it never ends inside one — which a
    /// token table cannot see, since the header's last token is an ordinary identifier.
    in_decl_header: bool,
    /// Did `last_emitted` follow a `.` / `?.`? Every clause and declaration keyword is
    /// also a legal property name, so `o.default`, `o.do`, and `o.class` reach the token
    /// tables spelled exactly like the statement forms. A member name ends an expression,
    /// and therefore may end a statement, whatever the word is.
    last_is_member_name: bool,
    /// Set by `case` / `default` and held through the label's `:`, which is the only
    /// `:` a statement block may follow. Every other `:` may introduce a member list.
    in_case_label: bool,
    /// True for exactly the token *after* a label's `:` — the state `classify_brace`
    /// reads to open a block rather than a value list.
    after_case_label_colon: bool,
    /// Would a `{` here open an import/export specifier list? Set by the `import` /
    /// `export` keyword and cleared by the first token that proves a declaration is
    /// being exported instead. The `type` modifier of `export type { … }` is
    /// contextual — lexed as an identifier — so the `{` is not always adjacent to the
    /// keyword that identifies it.
    brace_opens_specifier_list: bool,
}

impl<'a> Asi<'a> {
    /// ASI over `source`, attributing every span to `file` (see [`Lexer::new`]).
    pub fn new(source: &'a str, file: FileId) -> Self {
        Self {
            lexer: Lexer::new(source, file),
            buffered: None,
            lookahead: std::collections::VecDeque::new(),
            last_emitted: None,
            bracket_depth: 0,
            braces: Vec::new(),
            paren_is_header: Vec::new(),
            closed_header_paren: false,
            closed_block_brace: false,
            last_is_member_name: false,
            in_case_label: false,
            after_case_label_colon: false,
            in_decl_header: false,
            brace_opens_specifier_list: false,
            cast_angle_depth: 0,
            closed_cast_angle: false,
        }
    }

    pub fn next_token(&mut self) -> Token {
        if let Some(tok) = self.buffered.take() {
            self.track(&tok.kind);
            return tok;
        }

        let mut pending_newline = false;
        loop {
            let tok = self
                .lookahead
                .pop_front()
                .unwrap_or_else(|| self.lexer.next_token());
            match tok.kind {
                TokenKind::Newline => {
                    pending_newline = true;
                }
                TokenKind::Eof => {
                    if self.needs_semi() {
                        return self.insert_semicolon_before(tok);
                    }
                    self.track(&tok.kind);
                    return tok;
                }
                _ => {
                    // A statement ends at the `}` closing its block, newline or not
                    // (`function f() { return 1 }`); never at one closing a value list;
                    // anywhere else, only at a newline.
                    let boundary = match self.brace_closed_by(&tok.kind) {
                        Some(BraceKind::Value | BraceKind::MemberList) => false,
                        Some(BraceKind::Block) => true,
                        None => pending_newline,
                    };
                    if boundary
                        && !self.inside_brackets()
                        && !self.can_continue_here(&tok.kind)
                        && self.needs_semi()
                    {
                        return self.insert_semicolon_before(tok);
                    }
                    self.track(&tok.kind);
                    return tok;
                }
            }
        }
    }

    fn can_continue_here(&mut self, kind: &TokenKind) -> bool {
        if matches!(
            kind,
            TokenKind::Else
                | TokenKind::Catch
                | TokenKind::Finally
                | TokenKind::Extends
                | TokenKind::Implements
        ) {
            // Clause keywords can also name properties. Neither a clause nor a
            // declaration header can put `:` or `?` immediately after its keyword.
            loop {
                let next = self.lexer.next_token();
                let newline = matches!(next.kind, TokenKind::Newline);
                let member = matches!(next.kind, TokenKind::Colon | TokenKind::Question);
                self.lookahead.push_back(next);
                if !newline {
                    return !member;
                }
            }
        }
        can_continue(kind)
    }

    pub fn into_diagnostics(self) -> Vec<Diagnostic> {
        self.lexer.into_diagnostics()
    }

    /// Emit a zero-width `Semicolon` at `tok`'s start and hold `tok` for the next call.
    fn insert_semicolon_before(&mut self, tok: Token) -> Token {
        let pos = tok.span.start;
        let file = tok.span.file;
        self.buffered = Some(tok);
        let semi = Token::new(TokenKind::Semicolon, Span::new(file, pos, pos));
        self.track(&semi.kind);
        semi
    }

    fn needs_semi(&self) -> bool {
        // Two states no token table can see: the `)` closing a control-flow header is
        // followed by a body, and a declaration header runs on to its `{`.
        if self.closed_header_paren || self.in_decl_header {
            return false;
        }
        match &self.last_emitted {
            None => false,
            Some(kind) => self.last_is_member_name || !cannot_end_statement(kind),
        }
    }

    /// True where a statement may begin, so a keyword there is a keyword rather than a
    /// property name or an enum member.
    fn at_statement_start(&self) -> bool {
        matches!(
            self.last_emitted,
            None | Some(
                TokenKind::LeftBrace
                    | TokenKind::RightBrace
                    | TokenKind::Semicolon
                    | TokenKind::Colon
            )
        )
    }

    /// The kind of brace `next` would close, when it closes one. A `;` before a value-list
    /// `}` corrupts the object literal / pattern / import it closes; before a block `}` it
    /// is exactly the statement terminator JS's ASI supplies.
    fn brace_closed_by(&self, next: &TokenKind) -> Option<BraceKind> {
        if !matches!(next, TokenKind::RightBrace) {
            return None;
        }
        self.braces.last().map(|frame| frame.kind)
    }

    /// True inside a `(`, `[`, or `${` opened since the innermost block or member list.
    fn inside_brackets(&self) -> bool {
        self.bracket_depth > self.braces.last().map_or(0, |frame| frame.bracket_base)
    }

    /// Classify a `{` about to be emitted. Statement blocks follow a statement boundary;
    /// everything in expression/value position is a value list. A `:` is ambiguous, and
    /// `case`/`default` is the one label whose `:` introduces statements — an object
    /// value, a ternary's else branch, and a type annotation all use a member-list base.
    fn classify_brace(&self) -> BraceKind {
        if self.brace_opens_specifier_list {
            return BraceKind::Value;
        }
        if self.after_case_label_colon {
            return BraceKind::Block;
        }
        if self.closed_cast_angle {
            return BraceKind::Value;
        }
        match &self.last_emitted {
            // A colon also introduces object values and ternary alternates. Their
            // comma separators already suppress ASI, so the same base is safe.
            Some(TokenKind::Colon) => BraceKind::MemberList,
            Some(kind) if opens_value_brace(kind) => BraceKind::Value,
            _ => BraceKind::Block,
        }
    }

    fn push_brace(&mut self) {
        let kind = self.classify_brace();
        let bracket_base = match kind {
            BraceKind::Block | BraceKind::MemberList => self.bracket_depth,
            BraceKind::Value => self.braces.last().map_or(0, |frame| frame.bracket_base),
        };
        self.braces.push(BraceFrame { kind, bracket_base });
    }

    /// Advance every piece of state past `kind`.
    ///
    /// Ordering is load-bearing throughout: **every `next_*` below reads `self` as the
    /// state *before* `kind`**, which is why `last_emitted` is advanced last and why
    /// `after_case_label_colon` is computed before `in_case_label` overwrites itself.
    /// Bracket tracking runs first for the same reason — `push_brace` classifies the `{`
    /// from the pre-token state.
    fn track(&mut self, kind: &TokenKind) {
        self.correct_bang_context(kind);
        let closed_block = self.brace_closed_by(kind) == Some(BraceKind::Block);
        let closed_header = self.track_brackets(kind);
        let closed_cast = self.track_cast_angles(kind);
        self.after_case_label_colon = self.next_after_case_label_colon(kind);
        self.in_case_label = self.next_in_case_label(kind);
        self.in_decl_header = self.next_in_decl_header(kind);
        self.brace_opens_specifier_list = self.next_brace_opens_specifier_list(kind);
        self.last_is_member_name = self.next_last_is_member_name();
        self.closed_header_paren = closed_header;
        self.closed_block_brace = closed_block;
        self.closed_cast_angle = closed_cast;
        self.last_emitted = Some(kind.clone());
    }

    /// Angle-bracket bookkeeping for `<T>expr` casts; returns whether `kind` closed one.
    fn track_cast_angles(&mut self, kind: &TokenKind) -> bool {
        match kind {
            TokenKind::LessThan if self.cast_angle_depth > 0 => self.cast_angle_depth += 1,
            TokenKind::LessThan if self.expects_operand() => self.cast_angle_depth = 1,
            TokenKind::GreaterThan if self.cast_angle_depth > 0 => {
                self.cast_angle_depth -= 1;
                return self.cast_angle_depth == 0;
            }
            _ => {}
        }
        false
    }

    /// Is the next token an operand — the start of an expression rather than something
    /// continuing one?
    fn expects_operand(&self) -> bool {
        self.at_statement_start()
            || self
                .last_emitted
                .as_ref()
                // `=>` stays out of `opens_value_brace` because `=> {` opens a body, but
                // what follows it is still an operand: `x => <Foo>{ … }`.
                .is_some_and(|kind| opens_value_brace(kind) || matches!(kind, TokenKind::Arrow))
    }

    /// Refine the lexer's prefix/postfix choice using grammatical boundary state.
    fn correct_bang_context(&mut self, kind: &TokenKind) {
        if !matches!(kind, TokenKind::Bang) {
            return;
        }
        if self.last_is_member_name {
            self.lexer.set_bang_is_postfix(true);
        } else if self.closed_header_paren
            || self.closed_block_brace
            || matches!(self.last_emitted, Some(TokenKind::Semicolon))
        {
            self.lexer.set_bang_is_postfix(false);
        }
    }

    /// Bracket and brace bookkeeping; returns whether `kind` closed a control-flow header.
    fn track_brackets(&mut self, kind: &TokenKind) -> bool {
        match kind {
            TokenKind::LeftParen => {
                self.bracket_depth += 1;
                self.paren_is_header.push(self.opens_control_header());
            }
            // A `${` opens an expression the same way `(` does. `TemplateMiddle` closes one
            // substitution and opens the next, so it nets out to no change.
            TokenKind::LeftBracket | TokenKind::TemplateHead(_) => self.bracket_depth += 1,
            TokenKind::RightParen => {
                self.bracket_depth = self.bracket_depth.saturating_sub(1);
                return self.paren_is_header.pop().unwrap_or(false);
            }
            TokenKind::RightBracket | TokenKind::TemplateTail(_) => {
                self.bracket_depth = self.bracket_depth.saturating_sub(1);
            }
            TokenKind::LeftBrace => self.push_brace(),
            TokenKind::RightBrace => {
                self.braces.pop();
            }
            _ => {}
        }
        false
    }

    fn next_after_case_label_colon(&self, kind: &TokenKind) -> bool {
        matches!(kind, TokenKind::Colon) && self.in_case_label
    }

    fn next_in_case_label(&self, kind: &TokenKind) -> bool {
        match kind {
            // Both words are also legal property names, so a label needs both a statement
            // position and a `switch` body around it: `{ default: … }` has the first and
            // `o.default` neither.
            TokenKind::Case | TokenKind::Default => {
                self.at_statement_start()
                    && self.braces.last().map(|frame| frame.kind) == Some(BraceKind::Block)
            }
            // A label's expression may be several tokens (`case Color.Red:`), so the flag
            // survives to its `:`. Anything that ends a statement or a list ends the
            // search — the word was a name, not a label.
            TokenKind::Colon
            | TokenKind::Semicolon
            | TokenKind::Comma
            | TokenKind::LeftBrace
            | TokenKind::RightBrace
            | TokenKind::LeftParen
            | TokenKind::RightParen => false,
            _ => self.in_case_label,
        }
    }

    fn next_in_decl_header(&self, kind: &TokenKind) -> bool {
        match kind {
            // All three words are legal property names too, so a header needs a statement
            // position: `{ class: … }` and `o.class` are names.
            TokenKind::Class | TokenKind::Interface | TokenKind::Enum => {
                self.at_statement_start() || matches!(self.last_emitted, Some(TokenKind::Export))
            }
            TokenKind::LeftBrace
            | TokenKind::RightBrace
            | TokenKind::Semicolon
            | TokenKind::Colon
            | TokenKind::Comma => false,
            _ => self.in_decl_header,
        }
    }

    fn next_brace_opens_specifier_list(&self, kind: &TokenKind) -> bool {
        match kind {
            TokenKind::Import | TokenKind::Export => true,
            // Only the contextual `type` modifier may sit between the keyword and the
            // specifier list; any other token means a declaration is being exported.
            TokenKind::Identifier => self.brace_opens_specifier_list,
            _ => false,
        }
    }

    fn next_last_is_member_name(&self) -> bool {
        matches!(
            self.last_emitted,
            Some(TokenKind::Dot | TokenKind::QuestionDot)
        )
    }

    /// True when the `(` about to be tracked follows a control-flow keyword whose
    /// parenthesized header is followed by a body rather than a statement terminator.
    ///
    /// All five words are legal property names, so `r.switch(1)` would otherwise mark its
    /// *argument* list as a header and swallow the next statement's terminator.
    fn opens_control_header(&self) -> bool {
        !self.last_is_member_name
            && matches!(
                self.last_emitted,
                Some(
                    TokenKind::If
                        | TokenKind::While
                        | TokenKind::For
                        | TokenKind::Switch
                        | TokenKind::Catch
                )
            )
    }
}

// The five tables below split along an axis worth stating once: `cannot_end_statement`
// and the three it composes classify `last_emitted`, the token *behind* the newline;
// `can_continue` classifies the token *ahead* of it. That is why `else` and `finally`
// legitimately appear on both sides — they neither end the statement behind them nor
// start a new one ahead.

/// A statement cannot end on this token, so a following newline is mid-statement.
/// `,` always sits inside a value list (object literal, pattern, import, argument or
/// element list); an opening bracket has nothing before it to terminate; and an operator
/// or clause keyword is still waiting for what comes next.
fn cannot_end_statement(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Semicolon
            | TokenKind::LeftBrace
            | TokenKind::LeftParen
            | TokenKind::LeftBracket
            | TokenKind::Comma
    ) || awaits_operand(kind)
        || awaits_body(kind)
}

/// Operators that require a following operand, so an expression — and therefore a
/// statement — can never legally end on one, *and* whose operand is a value: the `{`
/// after one of these opens an object literal, not a block.
///
/// The narrower of the two tables, and the only one `opens_value_brace` may consult.
/// `needs_semi` and `can_continue` want [`awaits_operand`], the superset that also
/// covers `=>` — whose operand can be a statement block.
fn cannot_end_expression(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Equals
            | TokenKind::PlusEquals
            | TokenKind::MinusEquals
            | TokenKind::StarEquals
            | TokenKind::SlashEquals
            | TokenKind::PercentEquals
            | TokenKind::StarStarEquals
            | TokenKind::Plus
            | TokenKind::Minus
            | TokenKind::Star
            | TokenKind::Slash
            | TokenKind::Percent
            | TokenKind::StarStar
            | TokenKind::EqEqEq
            | TokenKind::EqEq
            | TokenKind::BangEqEq
            | TokenKind::BangEq
            | TokenKind::LessThan
            | TokenKind::GreaterThan
            | TokenKind::LessEquals
            | TokenKind::GreaterEquals
            | TokenKind::AmpAmp
            | TokenKind::Pipe
            | TokenKind::PipePipe
            | TokenKind::Question
            | TokenKind::QuestionQuestion
            | TokenKind::Colon
    )
}

/// Every operator that awaits an operand, so a newline after one is mid-expression:
/// [`cannot_end_expression`] plus `=>` — prettier's default wrap for a long arrow puts
/// a newline right there. `=>` is kept out of the narrower table because its operand
/// may be a statement block, which `opens_value_brace` must not read as a value.
fn awaits_operand(kind: &TokenKind) -> bool {
    cannot_end_expression(kind)
        || matches!(
            kind,
            TokenKind::Arrow
                // A class header is not a statement, so it never ends at one of its
                // own clause keywords — prettier wraps long headers right after them.
                | TokenKind::Extends
                | TokenKind::Implements
        )
}

/// Clause keywords whose body follows directly, so a statement can never end on one:
/// `try`, `else`, `do`, and `finally` — the Allman-brace shapes (`try` ⏎ `{`), plus
/// `else` ⏎ `if`.
///
/// Deliberately not folded into [`awaits_operand`]: `can_continue` reads that table, and
/// a newline *before* `try` or `do` does end the preceding statement.
fn awaits_body(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Try | TokenKind::Else | TokenKind::Do | TokenKind::Finally | TokenKind::Catch
    )
}

/// Tokens after which a `{` opens a value list rather than a statement block:
/// assignment/operator RHS, argument/element position, `return`/`throw` operands, a
/// template substitution's expression, and the `const`/`let` keywords that introduce
/// destructuring patterns. Import/export specifier lists are `brace_opens_specifier_list`'s job.
fn opens_value_brace(kind: &TokenKind) -> bool {
    // `>` is the one operand-awaiting token a `{` never follows as a value: `a > { … }`
    // compares against an object literal and is meaningless, while `function f():
    // Array<number> {` and `class Box<T> {` put a body there.
    if matches!(kind, TokenKind::GreaterThan) {
        return false;
    }
    cannot_end_expression(kind)
        || matches!(
            kind,
            TokenKind::LeftParen
                | TokenKind::LeftBracket
                | TokenKind::Comma
                // A spread's operand: `{ ...{ a: 1 } }`, `[...{ … }]`.
                | TokenKind::DotDotDot
                | TokenKind::Return
                | TokenKind::Throw
                | TokenKind::Const
                | TokenKind::Let
                | TokenKind::TemplateHead(_)
                | TokenKind::TemplateMiddle(_)
        )
}

/// Tokens that continue the statement behind a newline, so no terminator is inserted
/// before them.
///
/// Deliberately absent: `as` and `is`, contextual keywords lexed as identifiers, so a
/// newline before them ends the statement — matching TypeScript, where `const v = x` ⏎
/// `as (y)` is an initialized variable followed by a call of a function named `as`, not a
/// cast. `typeof` starts a new expression, so it is not listed either.
fn can_continue(kind: &TokenKind) -> bool {
    awaits_operand(kind)
        || matches!(
            kind,
            TokenKind::Dot
                // Prettier breaks a long member chain before `?.` exactly as before `.`.
                | TokenKind::QuestionDot
                | TokenKind::LeftParen
                | TokenKind::LeftBracket
                | TokenKind::Else
                // A `try`/`catch` block's `}` ends a clause, not the statement:
                // the clause keywords continue it exactly as `else` does.
                | TokenKind::Catch
                | TokenKind::Finally
                // Likewise a class header's own clause keywords, which prettier puts
                // on their own lines when the header is long.
                | TokenKind::Extends
                | TokenKind::Implements
                // `TemplateMiddle`/`TemplateTail` are always mid-template, never statement
                // starters. `TemplateHead`/`TemplateNoSubstitution` start expressions, so
                // are not listed.
                | TokenKind::TemplateMiddle(_)
                | TokenKind::TemplateTail(_)
        )
}

#[cfg(test)]
mod tests {
    use super::Asi;
    use crate::TokenKind;

    fn asi_kinds(source: &str) -> Vec<TokenKind> {
        let mut asi = Asi::new(source, crate::FileId(0));
        let mut kinds = Vec::new();
        loop {
            let tok = asi.next_token();
            let is_eof = tok.kind == TokenKind::Eof;
            kinds.push(tok.kind);
            if is_eof {
                break;
            }
        }
        let diags = asi.into_diagnostics();
        assert!(
            diags.is_empty(),
            "unexpected diagnostics for {source:?}: {diags:?}"
        );
        kinds
    }

    #[test]
    fn strips_all_newlines() {
        let kinds = asi_kinds("a\nb\n");
        assert!(!kinds.iter().any(|k| matches!(k, TokenKind::Newline)));
    }

    #[test]
    fn inserts_semicolon_between_statements() {
        assert_eq!(
            asi_kinds("a\nb"),
            vec![
                TokenKind::Identifier,
                TokenKind::Semicolon,
                TokenKind::Identifier,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn newline_before_as_splits_statement() {
        // `as` is a contextual keyword (a plain identifier here), so ASI ends the
        // statement — TS parses `a` ⏎ `as(b)` the same way.
        assert_eq!(
            asi_kinds("a\nas(b)"),
            vec![
                TokenKind::Identifier,
                TokenKind::Semicolon,
                TokenKind::Identifier,
                TokenKind::LeftParen,
                TokenKind::Identifier,
                TokenKind::RightParen,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn inserts_semicolon_before_close_brace() {
        assert_eq!(
            asi_kinds("{ a\n}"),
            vec![
                TokenKind::LeftBrace,
                TokenKind::Identifier,
                TokenKind::Semicolon,
                TokenKind::RightBrace,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn empty_block_no_semicolon_inside() {
        assert_eq!(
            asi_kinds("{\n}"),
            vec![
                TokenKind::LeftBrace,
                TokenKind::RightBrace,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn does_not_double_semicolon() {
        assert_eq!(
            asi_kinds("a;\nb;\n"),
            vec![
                TokenKind::Identifier,
                TokenKind::Semicolon,
                TokenKind::Identifier,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn continues_across_newline_with_infix_operator() {
        assert_eq!(
            asi_kinds("a = b\n+ c"),
            vec![
                TokenKind::Identifier,
                TokenKind::Equals,
                TokenKind::Identifier,
                TokenKind::Plus,
                TokenKind::Identifier,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn continues_across_newline_with_dot() {
        assert_eq!(
            asi_kinds("foo\n.bar"),
            vec![
                TokenKind::Identifier,
                TokenKind::Dot,
                TokenKind::Identifier,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn inserts_semicolon_at_eof() {
        assert_eq!(
            asi_kinds("a"),
            vec![TokenKind::Identifier, TokenKind::Semicolon, TokenKind::Eof,]
        );
    }

    #[test]
    fn eof_with_trailing_semicolon_no_double() {
        assert_eq!(
            asi_kinds("a;"),
            vec![TokenKind::Identifier, TokenKind::Semicolon, TokenKind::Eof,]
        );
    }

    #[test]
    fn no_semicolon_inside_parens() {
        assert_eq!(
            asi_kinds("foo(\n  a,\n  b\n)"),
            vec![
                TokenKind::Identifier,
                TokenKind::LeftParen,
                TokenKind::Identifier,
                TokenKind::Comma,
                TokenKind::Identifier,
                TokenKind::RightParen,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn no_semicolon_after_if_header_on_next_line() {
        // `if (a)` newline `b`: the `)` closes a control-flow header, so the body on the
        // next line must not be severed by an inserted `;`.
        assert_eq!(
            asi_kinds("if (a)\nb"),
            vec![
                TokenKind::If,
                TokenKind::LeftParen,
                TokenKind::Identifier,
                TokenKind::RightParen,
                TokenKind::Identifier,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn no_semicolon_after_while_header_on_next_line() {
        assert_eq!(
            asi_kinds("while (a)\nb"),
            vec![
                TokenKind::While,
                TokenKind::LeftParen,
                TokenKind::Identifier,
                TokenKind::RightParen,
                TokenKind::Identifier,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn header_paren_only_suppresses_its_own_close() {
        // The inner `)` of `if ((a))` is an ordinary paren; only the header `)` suppresses ASI.
        assert_eq!(
            asi_kinds("if ((a))\nb"),
            vec![
                TokenKind::If,
                TokenKind::LeftParen,
                TokenKind::LeftParen,
                TokenKind::Identifier,
                TokenKind::RightParen,
                TokenKind::RightParen,
                TokenKind::Identifier,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn call_close_paren_still_inserts_semicolon() {
        // A `)` closing a call expression ends a statement — ASI must still fire.
        assert_eq!(
            asi_kinds("f()\ng()"),
            vec![
                TokenKind::Identifier,
                TokenKind::LeftParen,
                TokenKind::RightParen,
                TokenKind::Semicolon,
                TokenKind::Identifier,
                TokenKind::LeftParen,
                TokenKind::RightParen,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn no_semicolon_inside_brackets() {
        assert_eq!(
            asi_kinds("[\n1,\n2\n]"),
            vec![
                TokenKind::LeftBracket,
                TokenKind::NumberLiteral(1.0),
                TokenKind::Comma,
                TokenKind::NumberLiteral(2.0),
                TokenKind::RightBracket,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn else_continues_if_statement() {
        assert_eq!(
            asi_kinds("if (c) { }\nelse { }"),
            vec![
                TokenKind::If,
                TokenKind::LeftParen,
                TokenKind::Identifier,
                TokenKind::RightParen,
                TokenKind::LeftBrace,
                TokenKind::RightBrace,
                TokenKind::Else,
                TokenKind::LeftBrace,
                TokenKind::RightBrace,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn catch_continues_try_statement() {
        assert_eq!(
            asi_kinds("try { }\ncatch (e) { }"),
            vec![
                TokenKind::Try,
                TokenKind::LeftBrace,
                TokenKind::RightBrace,
                TokenKind::Catch,
                TokenKind::LeftParen,
                TokenKind::Identifier,
                TokenKind::RightParen,
                TokenKind::LeftBrace,
                TokenKind::RightBrace,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn finally_continues_try_statement() {
        assert_eq!(
            asi_kinds("try { }\nfinally { }"),
            vec![
                TokenKind::Try,
                TokenKind::LeftBrace,
                TokenKind::RightBrace,
                TokenKind::Finally,
                TokenKind::LeftBrace,
                TokenKind::RightBrace,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn no_semicolon_after_trailing_arrow() {
        // Prettier's default wrap for a long arrow: `=>` awaits its body, so the
        // newline after it must not be turned into a statement end.
        assert_eq!(
            asi_kinds("const f = (x: number): number =>\nx + 1"),
            vec![
                TokenKind::Const,
                TokenKind::Identifier,
                TokenKind::Equals,
                TokenKind::LeftParen,
                TokenKind::Identifier,
                TokenKind::Colon,
                TokenKind::Identifier,
                TokenKind::RightParen,
                TokenKind::Colon,
                TokenKind::Identifier,
                TokenKind::Arrow,
                TokenKind::Identifier,
                TokenKind::Plus,
                TokenKind::NumberLiteral(1.0),
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn arrow_block_body_still_gets_terminator() {
        // Regression guard for the `awaits_operand` split: `=>` must not make the
        // body brace a value list, or the last statement loses its inserted `;`.
        assert_eq!(
            asi_kinds("const f = () => {\na\n}"),
            vec![
                TokenKind::Const,
                TokenKind::Identifier,
                TokenKind::Equals,
                TokenKind::LeftParen,
                TokenKind::RightParen,
                TokenKind::Arrow,
                TokenKind::LeftBrace,
                TokenKind::Identifier,
                TokenKind::Semicolon,
                TokenKind::RightBrace,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn extends_continues_class_header() {
        assert_eq!(
            asi_kinds("class S\nextends B { }"),
            vec![
                TokenKind::Class,
                TokenKind::Identifier,
                TokenKind::Extends,
                TokenKind::Identifier,
                TokenKind::LeftBrace,
                TokenKind::RightBrace,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn no_semicolon_after_trailing_extends() {
        assert_eq!(
            asi_kinds("class S extends\nB { }"),
            vec![
                TokenKind::Class,
                TokenKind::Identifier,
                TokenKind::Extends,
                TokenKind::Identifier,
                TokenKind::LeftBrace,
                TokenKind::RightBrace,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn implements_continues_class_header() {
        assert_eq!(
            asi_kinds("class S\nimplements I { }"),
            vec![
                TokenKind::Class,
                TokenKind::Identifier,
                TokenKind::Implements,
                TokenKind::Identifier,
                TokenKind::LeftBrace,
                TokenKind::RightBrace,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn no_semicolon_after_trailing_implements() {
        assert_eq!(
            asi_kinds("class S implements\nI { }"),
            vec![
                TokenKind::Class,
                TokenKind::Identifier,
                TokenKind::Implements,
                TokenKind::Identifier,
                TokenKind::LeftBrace,
                TokenKind::RightBrace,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn optional_chain_continues_across_newline() {
        // Prettier breaks a long member chain before `?.` exactly as before `.`.
        assert_eq!(
            asi_kinds("foo\n?.bar"),
            vec![
                TokenKind::Identifier,
                TokenKind::QuestionDot,
                TokenKind::Identifier,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn empty_input() {
        assert_eq!(asi_kinds(""), vec![TokenKind::Eof]);
    }

    #[test]
    fn leading_newlines_no_semicolon() {
        assert_eq!(
            asi_kinds("\n\na"),
            vec![TokenKind::Identifier, TokenKind::Semicolon, TokenKind::Eof,]
        );
    }

    #[test]
    fn no_semicolon_in_multiline_object_literal() {
        // `= {` opens a value brace; the comma rule covers inter-member newlines and the
        // value-brace rule covers the newline before `}`.
        assert_eq!(
            asi_kinds("x = {\na: 1,\nb: 2\n}"),
            vec![
                TokenKind::Identifier,
                TokenKind::Equals,
                TokenKind::LeftBrace,
                TokenKind::Identifier,
                TokenKind::Colon,
                TokenKind::NumberLiteral(1.0),
                TokenKind::Comma,
                TokenKind::Identifier,
                TokenKind::Colon,
                TokenKind::NumberLiteral(2.0),
                TokenKind::RightBrace,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn no_semicolon_in_multiline_object_literal_trailing_comma() {
        assert_eq!(
            asi_kinds("x = {\na: 1,\n}"),
            vec![
                TokenKind::Identifier,
                TokenKind::Equals,
                TokenKind::LeftBrace,
                TokenKind::Identifier,
                TokenKind::Colon,
                TokenKind::NumberLiteral(1.0),
                TokenKind::Comma,
                TokenKind::RightBrace,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn block_brace_still_gets_terminator() {
        // A statement-position `{` is a block; its last statement keeps its inserted `;`.
        assert_eq!(
            asi_kinds("{\na\n}"),
            vec![
                TokenKind::LeftBrace,
                TokenKind::Identifier,
                TokenKind::Semicolon,
                TokenKind::RightBrace,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn type_literal_members_get_separators() {
        // A `: {` type literal is a value list, so its closing `}` gets no terminator —
        // the member grammar makes the last separator optional. Newline-separated members
        // still receive an inserted `;` between them.
        assert_eq!(
            asi_kinds("let p: {\na: number\nb: number\n}"),
            vec![
                TokenKind::Let,
                TokenKind::Identifier,
                TokenKind::Colon,
                TokenKind::LeftBrace,
                TokenKind::Identifier,
                TokenKind::Colon,
                TokenKind::Identifier,
                TokenKind::Semicolon,
                TokenKind::Identifier,
                TokenKind::Colon,
                TokenKind::Identifier,
                TokenKind::RightBrace,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn nested_object_value_suppresses_inner_close() {
        // The inner `{` follows `:` inside a value brace, so it is itself a value brace.
        assert_eq!(
            asi_kinds("x = {\nouter: {\ninner: 1\n}\n}"),
            vec![
                TokenKind::Identifier,
                TokenKind::Equals,
                TokenKind::LeftBrace,
                TokenKind::Identifier,
                TokenKind::Colon,
                TokenKind::LeftBrace,
                TokenKind::Identifier,
                TokenKind::Colon,
                TokenKind::NumberLiteral(1.0),
                TokenKind::RightBrace,
                TokenKind::RightBrace,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn multiline_destructuring_pattern_no_semicolon() {
        assert_eq!(
            asi_kinds("const {\na,\nb\n} = p"),
            vec![
                TokenKind::Const,
                TokenKind::LeftBrace,
                TokenKind::Identifier,
                TokenKind::Comma,
                TokenKind::Identifier,
                TokenKind::RightBrace,
                TokenKind::Equals,
                TokenKind::Identifier,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn no_semicolon_after_trailing_equals() {
        // `const x =` newline `5`: the `=` cannot end an expression, so the initializer
        // continues onto the next line rather than being severed by an inserted `;`.
        assert_eq!(
            asi_kinds("const x =\n5"),
            vec![
                TokenKind::Const,
                TokenKind::Identifier,
                TokenKind::Equals,
                TokenKind::NumberLiteral(5.0),
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn no_semicolon_after_trailing_binary_operator() {
        assert_eq!(
            asi_kinds("const x = 1 +\n2"),
            vec![
                TokenKind::Const,
                TokenKind::Identifier,
                TokenKind::Equals,
                TokenKind::NumberLiteral(1.0),
                TokenKind::Plus,
                TokenKind::NumberLiteral(2.0),
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn no_semicolon_after_trailing_assignment_reassign() {
        assert_eq!(
            asi_kinds("x =\n5"),
            vec![
                TokenKind::Identifier,
                TokenKind::Equals,
                TokenKind::NumberLiteral(5.0),
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn no_semicolon_after_trailing_compound_assignment() {
        assert_eq!(
            asi_kinds("x +=\n5"),
            vec![
                TokenKind::Identifier,
                TokenKind::PlusEquals,
                TokenKind::NumberLiteral(5.0),
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn no_semicolon_after_trailing_logical_operator() {
        assert_eq!(
            asi_kinds("true &&\nfalse"),
            vec![
                TokenKind::BooleanLiteral(true),
                TokenKind::AmpAmp,
                TokenKind::BooleanLiteral(false),
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn no_semicolon_after_trailing_nullish_coalescing() {
        assert_eq!(
            asi_kinds("a ??\nb"),
            vec![
                TokenKind::Identifier,
                TokenKind::QuestionQuestion,
                TokenKind::Identifier,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn no_semicolon_before_leading_ternary() {
        // `cond` newline `? 1 : 2`: a continuation line beginning with `?` must not be
        // severed from the condition.
        assert_eq!(
            asi_kinds("cond\n? 1 : 2"),
            vec![
                TokenKind::Identifier,
                TokenKind::Question,
                TokenKind::NumberLiteral(1.0),
                TokenKind::Colon,
                TokenKind::NumberLiteral(2.0),
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn no_semicolon_after_trailing_ternary_question_and_colon() {
        assert_eq!(
            asi_kinds("cond ?\n1 :\n2"),
            vec![
                TokenKind::Identifier,
                TokenKind::Question,
                TokenKind::NumberLiteral(1.0),
                TokenKind::Colon,
                TokenKind::NumberLiteral(2.0),
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn no_semicolon_in_multiline_operator_chain() {
        assert_eq!(
            asi_kinds("const x = a +\nb +\nc"),
            vec![
                TokenKind::Const,
                TokenKind::Identifier,
                TokenKind::Equals,
                TokenKind::Identifier,
                TokenKind::Plus,
                TokenKind::Identifier,
                TokenKind::Plus,
                TokenKind::Identifier,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn return_keeps_restricted_production() {
        // Regression guard: `return` newline `5` is the restricted production — ASI inserts
        // `;` after `return`, leaving the `5` as its own statement.
        assert_eq!(
            asi_kinds("return\n5"),
            vec![
                TokenKind::Return,
                TokenKind::Semicolon,
                TokenKind::NumberLiteral(5.0),
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    /// `source` with `<;>` marking every offset where ASI inserted a semicolon — always
    /// immediately before the token that triggered it, since an inserted terminator is
    /// zero-width at that token's start.
    ///
    /// Use this to pin *where* a terminator lands; use an `asi_kinds` token vector when
    /// the whole stream matters — that nothing else was dropped, reordered, or emitted.
    fn asi_marked(source: &str) -> String {
        let mut asi = Asi::new(source, crate::FileId(0));
        let mut inserted: Vec<usize> = Vec::new();
        loop {
            let tok = asi.next_token();
            if tok.kind == TokenKind::Eof {
                break;
            }
            // An inserted semicolon is zero-width; one the author wrote spans its `;`.
            if tok.kind == TokenKind::Semicolon && tok.span.start == tok.span.end {
                inserted.push(tok.span.start as usize);
            }
        }
        let mut out = String::new();
        let mut last = 0;
        for at in inserted {
            out.push_str(&source[last..at]);
            out.push_str("<;>");
            last = at;
        }
        out.push_str(&source[last..]);
        out
    }

    #[test]
    fn clause_keyword_before_its_brace_gets_no_terminator() {
        // Allman style: `try` / `catch (…)` / `else` / `do` are followed by a body, so
        // the newline before the `{` is not a statement end.
        assert_eq!(
            asi_marked("try\n{\na\n}\ncatch (e)\n{\n}"),
            "try\n{\na\n<;>}\ncatch (e)\n{\n}<;>"
        );
        assert_eq!(
            asi_marked("if (c)\n{\n}\nelse\n{\n}"),
            "if (c)\n{\n}\nelse\n{\n}<;>"
        );
        // ECMAScript inserts nothing before this `while` — the DoWhileStatement production
        // allows it — but a token-stream pre-pass cannot see that, so it emits one here and
        // the parser drops it (`eat_asi_semicolon_before`). The rows below record where the
        // pre-pass lands the terminator; the behaviour that matters is pinned by
        // `asi_allman_braces.ts`.
        assert_eq!(asi_marked("do\n{\n}\nwhile (c)"), "do\n{\n}\n<;>while (c)");
    }

    #[test]
    fn a_newline_before_a_clause_keyword_still_ends_the_statement() {
        // `awaits_body` must not have leaked into `can_continue`: a newline *before*
        // `try` or `do` ends the preceding statement.
        assert_eq!(asi_marked("a\ntry\n{\n}"), "a\n<;>try\n{\n}<;>");
        assert_eq!(
            asi_marked("a\ndo\n{\n}\nwhile (c)"),
            "a\n<;>do\n{\n}\n<;>while (c)"
        );
    }

    #[test]
    fn block_closing_brace_terminates_without_a_newline() {
        assert_eq!(
            asi_marked("function f(): number { return 1 }"),
            "function f(): number { return 1 <;>}<;>"
        );
        assert_eq!(
            asi_marked("class C { x: number = 1 }"),
            "class C { x: number = 1 <;>}<;>"
        );
    }

    #[test]
    fn value_closing_brace_never_terminates() {
        assert_eq!(asi_marked("x = { a: 1 }"), "x = { a: 1 }<;>");
        assert_eq!(
            asi_marked("import { a } from \"m\""),
            "import { a } from \"m\"<;>"
        );
        // The contextual `type` modifier sits between `export` and its specifier list.
        assert_eq!(
            asi_marked("export type { A } from \"m\""),
            "export type { A } from \"m\"<;>"
        );
    }

    #[test]
    fn only_a_after_case_label_colon_opens_a_block() {
        // The `:` of a `case`/`default` label is the one colon a statement block follows;
        // a ternary's alternate and an object value after the same two words are not.
        assert_eq!(
            asi_marked("switch (n) {\ncase 1: { a }\n}"),
            "switch (n) {\ncase 1: { a <;>}\n<;>}<;>"
        );
        assert_eq!(
            asi_marked("x = c ? { a: 1 } : { a: 2 }"),
            "x = c ? { a: 1 } : { a: 2 }<;>"
        );
        assert_eq!(
            asi_marked("x = { default: { a: 1 } }"),
            "x = { default: { a: 1 } }<;>"
        );
    }

    #[test]
    fn a_keyword_property_name_is_not_a_keyword() {
        // Every clause and declaration keyword is also a legal property name, so the
        // tables must not read `o.do` as a `do` loop or `{ class: … }` as a class header.
        assert_eq!(asi_marked("a = o.do\nb = 1"), "a = o.do\n<;>b = 1<;>");
        assert_eq!(asi_marked("a = o.class\nb = 1"), "a = o.class\n<;>b = 1<;>");
        assert_eq!(
            asi_marked("a = o.default\nb = 1"),
            "a = o.default\n<;>b = 1<;>"
        );
        assert_eq!(asi_marked("a = o?.try\nb = 1"), "a = o?.try\n<;>b = 1<;>");
        // A call's argument list is not a control-flow header just because the method is
        // spelled `switch`.
        assert_eq!(
            asi_marked("a = o.switch(1)\nb = 1"),
            "a = o.switch(1)\n<;>b = 1<;>"
        );
        assert_eq!(
            asi_marked("a = o.catch(1)\nb = 1"),
            "a = o.catch(1)\n<;>b = 1<;>"
        );
    }

    #[test]
    fn a_generic_header_is_followed_by_a_block() {
        // `>` closes a type-argument list here, so the `{` opens a body, not a value.
        assert_eq!(
            asi_marked("function f(): Array<number> {\nreturn a\n}"),
            "function f(): Array<number> {\nreturn a\n<;>}<;>"
        );
        assert_eq!(
            asi_marked("class Box<T> {\nv: number = 1\n}"),
            "class Box<T> {\nv: number = 1\n<;>}<;>"
        );
    }

    #[test]
    fn a_cast_is_followed_by_an_object_literal() {
        // A `<` in operand position opens a cast, so its `>` leaves a value next.
        assert_eq!(
            asi_marked("x = <Foo>{ a: 1 }\ny()"),
            "x = <Foo>{ a: 1 }\n<;>y()<;>"
        );
        assert_eq!(asi_marked("<Foo>{ a: 1 }\ny()"), "<Foo>{ a: 1 }\n<;>y()<;>");
        assert_eq!(
            asi_marked("f = x => <Foo>{ a: x }\ny()"),
            "f = x => <Foo>{ a: x }\n<;>y()<;>"
        );
        assert_eq!(
            asi_marked("x = <Box<Array<number>>>{ v: [] }\ny()"),
            "x = <Box<Array<number>>>{ v: [] }\n<;>y()<;>"
        );
        // After a comparison the next `<` follows an operand, so no cast is tracked.
        assert_eq!(
            asi_marked("if (a < b) {\nreturn a\n}"),
            "if (a < b) {\nreturn a\n<;>}<;>"
        );
    }

    #[test]
    fn a_spread_operand_is_an_object_literal() {
        assert_eq!(
            asi_marked("x = { ...{ a: 1 }, b: 2 }\ny()"),
            "x = { ...{ a: 1 }, b: 2 }\n<;>y()<;>"
        );
    }

    #[test]
    fn template_substitution_suspends_insertion() {
        assert_eq!(
            asi_marked("const s = `a${\nn\n}`"),
            "const s = `a${\nn\n}`<;>"
        );
        assert_eq!(
            asi_marked("const s = `a${\nn\n}b${\nn\n}c`"),
            "const s = `a${\nn\n}b${\nn\n}c`<;>"
        );
    }

    #[test]
    fn declaration_header_suspends_insertion() {
        assert_eq!(
            asi_marked("class S\nextends B\nimplements I\n{\n}"),
            "class S\nextends B\nimplements I\n{\n}<;>"
        );
        assert_eq!(asi_marked("interface I\n{\n}"), "interface I\n{\n}<;>");
        assert_eq!(asi_marked("enum E\n{\nA\n}"), "enum E\n{\nA\n<;>}<;>");
    }

    #[test]
    fn block_inside_parens_is_statement_context_again() {
        // The body of an arrow passed as an argument gets its terminators, while the
        // argument list around it stays suspended.
        assert_eq!(
            asi_marked("f((x: number): number => {\nreturn x\n})"),
            "f((x: number): number => {\nreturn x\n<;>})<;>"
        );
        assert_eq!(asi_marked("f(a,\nb)"), "f(a,\nb)<;>");
    }

    #[test]
    fn mvp_fixture_snapshot() {
        let source = "function greet(name: string): string {\n  return \"Hello, \" + name;\n}\n\nfunction main(): string {\n  const msg = greet(\"world\");\n  console.log(msg);\n  assert(msg === \"Hello, world\", \"greeting should match\");\n  return msg;\n}\n";
        let mut asi = Asi::new(source, crate::FileId(0));
        let mut tokens = Vec::new();
        loop {
            let tok = asi.next_token();
            let is_eof = tok.kind == TokenKind::Eof;
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        assert!(asi.into_diagnostics().is_empty());
        let kinds: Vec<String> = tokens.iter().map(|t| format!("{:?}", t.kind)).collect();
        insta::assert_debug_snapshot!(kinds);
    }
}
