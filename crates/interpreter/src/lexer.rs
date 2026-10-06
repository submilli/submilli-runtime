use crate::compiler_error::{CompileError, CompilerFailure, CompilerStage};
use num_bigint::BigUint;
use num_traits::Num;
use unicode_ident::{is_xid_continue, is_xid_start};

use crate::{Diagnostic, FileId, RawDoc, Severity, Span, Token, TokenKind};

const VALID_ESCAPES: &str =
    "valid escapes: \\\", \\', \\\\, \\/, \\n, \\t, \\r, \\b, \\f, \\v, \\0, \\u{HHHH}";

/// UTF-8 encoding of U+FEFF, which many editors write at the start of a file.
const BOM: &[u8] = "\u{feff}".as_bytes();

pub struct Lexer<'a> {
    source: &'a str,
    bytes: &'a [u8],
    file: FileId,
    pos: u32,
    diagnostics: Vec<Diagnostic>,
    fatal: Option<CompilerFailure>,
    /// Only the last doc comment before a declaration attaches (JSDoc convention); earlier ones are dropped.
    pending_docs: Vec<RawDoc>,
    /// Brace depth for each active `${ … }` interpolation. A `}` with the top
    /// frame at `0` closes the interpolation and resumes template scanning;
    /// nested templates push additional frames.
    template_frames: Vec<u32>,
    /// Previous non-newline token, used to disambiguate `/` as regex literal vs. division.
    last_significant_token: Option<TokenKind>,
    last_bang_is_postfix: bool,
}

impl<'a> Lexer<'a> {
    /// Lex `source`, attributing every span to `file`. The caller owns the
    /// [`FileId`] (it indexes into the [`Sources`](crate::Sources) registry):
    /// single-file scripts pass the script's id, multi-file packages pass each
    /// module's.
    pub fn new(source: &'a str, file: FileId) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            file,
            // A leading U+FEFF is an encoding marker, not source text. It is skipped by
            // advancing past it rather than trimming `source`, so every span stays a byte
            // offset into the file as it exists on disk and carets remain accurate.
            // Offset 0 only: anywhere else U+FEFF is a real character and still an error.
            pos: if source.as_bytes().starts_with(BOM) {
                BOM.len() as u32
            } else {
                0
            },
            diagnostics: Vec::new(),
            fatal: (source.len() > (u32::MAX - 4) as usize).then(|| CompilerFailure::Limit {
                stage: CompilerStage::Parse,
                span: None,
                message: "source exceeds the 32-bit lexer offset limit".into(),
                help: vec!["split the source into smaller modules".into()],
            }),
            pending_docs: Vec::new(),
            template_frames: Vec::new(),
            last_significant_token: None,
            last_bang_is_postfix: false,
        }
    }

    fn span(&self, start: u32, end: u32) -> Span {
        Span {
            file: self.file,
            start,
            end,
        }
    }

    pub fn next_token(&mut self) -> Token {
        let token = self.next_token_inner();
        if self.fatal.is_none()
            && let Err(error) = token.span.text(self.source, self.file)
        {
            self.fatal = Some(error.into_compiler_failure(CompilerStage::Parse));
            return self.eof_token();
        }
        token
    }

    fn next_token_inner(&mut self) -> Token {
        if self.fatal.is_some() {
            return self.eof_token();
        }
        loop {
            self.skip_trivia();
            let Some(b) = self.peek() else {
                let tok = self.eof_token();
                return self.finalize(tok);
            };
            let tok = match b {
                // Returns early to bypass finalize so pending_docs survive intervening newlines.
                b'\n' | b'\r' => return self.lex_newline(),
                b'0'..=b'9' => self.lex_number(),
                b'.' if self.digit_at(1) => self.lex_number(),
                b'"' | b'\'' => self.lex_string(b),
                b'`' => {
                    let start = self.pos;
                    self.pos += 1;
                    self.lex_template_part(start, true)
                }
                b'=' | b'!' | b'<' | b'>' | b'+' | b'-' | b'*' | b'/' | b'%' | b'.' => {
                    self.lex_operator()
                }
                b'&' if self.peek_at(1) == Some(b'&') => self.lex_operator(),
                b'|' => self.lex_operator(),
                b'(' | b')' | b'{' | b'}' | b'[' | b']' | b',' | b':' | b';' | b'?' => {
                    self.lex_delimiter()
                }
                _ => {
                    if b.is_ascii_alphabetic() || b == b'_' || b == b'$' {
                        self.lex_ident()
                    } else if b >= 0x80
                        && let Some(c) = self.peek_char()
                    {
                        if is_xid_start(c) {
                            self.lex_ident()
                        } else {
                            self.diagnose_unexpected_char(c);
                            self.pos += c.len_utf8() as u32;
                            continue;
                        }
                    } else {
                        self.diagnose_unexpected_byte(b);
                        self.pos += 1;
                        continue;
                    }
                }
            };
            return self.finalize(tok);
        }
    }

    fn finalize(&mut self, tok: Token) -> Token {
        let tok = self.attach_doc(tok);
        match &tok.kind {
            TokenKind::Newline => {}
            other => {
                self.last_bang_is_postfix =
                    matches!(other, TokenKind::Bang) && !self.regex_context();
                self.last_significant_token = Some(other.clone());
            }
        }
        tok
    }

    /// ASI knows statement boundaries and keyword property names that the raw
    /// token table cannot distinguish. Apply its correction before lexing `/`.
    pub(crate) fn set_bang_is_postfix(&mut self, postfix: bool) {
        self.last_bang_is_postfix = postfix;
    }

    fn regex_context(&self) -> bool {
        !self.last_bang_is_postfix && is_regex_context(&self.last_significant_token)
    }

    pub fn into_diagnostics(self) -> Vec<Diagnostic> {
        let file = self.file;
        self.finish()
            .unwrap_or_else(|error| error.into_diagnostics(file))
    }

    pub fn finish(self) -> Result<Vec<Diagnostic>, CompileError> {
        match self.fatal {
            Some(fatal) => Err(CompileError {
                diagnostics: self.diagnostics,
                fatal: Some(fatal),
            }),
            None => Ok(self.diagnostics),
        }
    }

    fn fail(&mut self, message: &str) -> Token {
        self.fatal.get_or_insert_with(|| CompilerFailure::Internal {
            stage: CompilerStage::Parse,
            span: None,
            message: message.into(),
        });
        self.eof_token()
    }

    fn peek(&self) -> Option<u8> {
        if self.fatal.is_some() {
            return None;
        }
        self.bytes.get(self.pos as usize).copied()
    }

    fn digit_at(&self, offset: usize) -> bool {
        self.peek_at(offset).is_some_and(|b| b.is_ascii_digit())
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        (self.pos as usize)
            .checked_add(offset)
            .and_then(|index| self.bytes.get(index))
            .copied()
    }

    fn peek_char(&self) -> Option<char> {
        self.source.get(self.pos as usize..)?.chars().next()
    }

    fn eof_token(&self) -> Token {
        Token::new(TokenKind::Eof, self.span(self.pos, self.pos))
    }

    fn error(&mut self, span: Span, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            severity: Severity::Error,
            span,
            message: message.into(),
            help: vec![],
            notes: vec![],
        });
    }

    fn error_with_help(&mut self, span: Span, message: impl Into<String>, help: Vec<String>) {
        self.diagnostics.push(Diagnostic {
            severity: Severity::Error,
            span,
            message: message.into(),
            help,
            notes: vec![],
        });
    }

    fn diagnose_unexpected_byte(&mut self, b: u8) {
        let span = self.span(self.pos, self.pos + 1);
        let message = if b.is_ascii() && !b.is_ascii_control() {
            format!("unexpected character `{}`", b as char)
        } else {
            format!("unexpected byte 0x{b:02X}")
        };
        self.error(span, message);
    }

    fn diagnose_unexpected_char(&mut self, c: char) {
        let len = c.len_utf8() as u32;
        self.error(
            self.span(self.pos, self.pos + len),
            format!("unexpected character `{c}`"),
        );
    }

    fn skip_trivia(&mut self) {
        loop {
            match self.peek() {
                // Space, tab, vertical tab, form feed.
                Some(b' ' | b'\t' | 0x0B | 0x0C) => self.pos += 1,
                Some(0x80..) if self.skip_space_separator() => {}
                Some(b'/') => match self.peek_at(1) {
                    Some(b'/') => self.skip_line_comment(),
                    Some(b'*') => {
                        // `/**/` is a regular empty block comment; doc requires `/**` + non-`/` next.
                        if self.peek_at(2) == Some(b'*') && self.peek_at(3) != Some(b'/') {
                            self.capture_doc_comment();
                        } else {
                            self.skip_block_comment();
                        }
                    }
                    _ => return,
                },
                _ => return,
            }
        }
    }

    /// Skips one non-ASCII space separator; false when the next character is not one.
    fn skip_space_separator(&mut self) -> bool {
        let Some(c) = self.peek_char().filter(|&c| is_space_separator(c)) else {
            return false;
        };
        self.pos += c.len_utf8() as u32;
        true
    }

    fn skip_line_comment(&mut self) {
        self.pos += 2;
        while let Some(b) = self.peek() {
            if matches!(b, b'\n' | b'\r') {
                break;
            }
            self.pos += 1;
        }
    }

    fn skip_block_comment(&mut self) {
        let start = self.pos;
        self.pos += 2;
        loop {
            match self.peek() {
                None => {
                    self.error(self.span(start, start + 2), "unterminated block comment");
                    return;
                }
                Some(b'*') if self.peek_at(1) == Some(b'/') => {
                    self.pos += 2;
                    return;
                }
                Some(_) => self.pos += 1,
            }
        }
    }

    /// Captures a `/** ... */` comment into `pending_docs`. Stored text includes delimiters.
    fn capture_doc_comment(&mut self) {
        let start = self.pos;
        self.pos += 3; // past `/**`
        loop {
            match self.peek() {
                None => {
                    self.error(self.span(start, start + 3), "unterminated block comment");
                    return;
                }
                Some(b'*') if self.peek_at(1) == Some(b'/') => {
                    self.pos += 2;
                    let span = self.span(start, self.pos);
                    let Some(text) = self.source.get(start as usize..self.pos as usize) else {
                        self.fail("invalid documentation span");
                        return;
                    };
                    let text = text.to_string();
                    self.pending_docs.push(RawDoc { text, span });
                    return;
                }
                Some(_) => self.pos += 1,
            }
        }
    }

    fn attach_doc(&mut self, mut tok: Token) -> Token {
        if !matches!(tok.kind, TokenKind::Newline) {
            tok.leading_doc = self.pending_docs.pop();
            self.pending_docs.clear();
        }
        tok
    }

    // next_token_inner matches the current byte immediately before dispatch.
    fn lex_newline(&mut self) -> Token {
        let start = self.pos;
        match self.peek() {
            Some(b'\r') => {
                self.pos += 1;
                if self.peek() == Some(b'\n') {
                    self.pos += 1;
                }
            }
            Some(b'\n') => self.pos += 1,
            _ => unreachable!("newline dispatch requires a newline byte"),
        }
        Token::new(TokenKind::Newline, self.span(start, self.pos))
    }

    /// Entered at a digit, or at a `.` followed by a digit (`.5`), whose integer
    /// part is then empty.
    fn lex_number(&mut self) -> Token {
        let start = self.pos;

        if self.peek() == Some(b'0')
            && let Some(radix) = self.peek_at(1).and_then(Radix::from_prefix)
        {
            return self.lex_radix_number(start, radix);
        }

        let token = self.lex_decimal_number(start);
        if matches!(
            token.kind,
            TokenKind::NumberLiteral(_) | TokenKind::BigIntLiteral(_)
        ) {
            self.reject_leading_zero(token.span);
        }
        token
    }

    fn lex_decimal_number(&mut self, start: u32) -> Token {
        let mut has_fraction_or_exponent = false;

        self.scan_digits(|b| b.is_ascii_digit());

        let Some(int_part) = self.source.get(start as usize..self.pos as usize) else {
            return self.fail("invalid numeric literal span");
        };
        if self.peek() == Some(b'.') {
            match self.decimal_point_role(int_part) {
                DecimalPoint::Literal => {
                    has_fraction_or_exponent = true;
                    self.pos += 1;
                    self.scan_digits(|b| b.is_ascii_digit());
                }
                DecimalPoint::MemberAccess => {}
                DecimalPoint::NameAfter => self.report_name_after_decimal_point(start, int_part),
            }
        }

        if matches!(self.peek(), Some(b'e' | b'E')) {
            has_fraction_or_exponent = true;
            let exp_start = self.pos;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            let digits_start = self.pos;
            self.scan_digits(|b| b.is_ascii_digit());
            if digits_start == self.pos {
                self.error(self.span(exp_start, self.pos), "missing digits in exponent");
                return Token::new(
                    TokenKind::NumberLiteral(f64::NAN),
                    self.span(start, self.pos),
                );
            }
        }

        if self.peek() == Some(b'n') {
            self.pos += 1;
            let span = self.span(start, self.pos);
            if has_fraction_or_exponent {
                self.error(
                    span,
                    "bigint literal cannot have a fractional or exponent part; \
                     remove the `.` / exponent or drop the `n` suffix",
                );
            }
            // Without a fraction or exponent the digits are `int_part`; with one,
            // keeping the integer prefix is the recovery.
            return Token::new(TokenKind::BigIntLiteral(without_separators(int_part)), span);
        }

        let span = self.span(start, self.pos);
        let Some(lexeme) = self.source.get(start as usize..self.pos as usize) else {
            return self.fail("invalid token source span");
        };
        let lexeme = without_separators(lexeme);
        let value = if let Ok(v) = lexeme.parse::<f64>() {
            v
        } else {
            self.error(span, format!("invalid number literal `{lexeme}`"));
            f64::NAN
        };

        Token::new(TokenKind::NumberLiteral(value), span)
    }

    /// What the `.` at the current position, after the integer part `int_part`, is.
    fn decimal_point_role(&self, int_part: &str) -> DecimalPoint {
        if self.digit_at(1) {
            return DecimalPoint::Literal;
        }
        if is_legacy_octal_digits(int_part) {
            return DecimalPoint::MemberAccess;
        }
        if !self.identifier_starts_at(1) || self.literal_continues_after_point() {
            return DecimalPoint::Literal;
        }
        DecimalPoint::NameAfter
    }

    /// After `1.`, an exponent (`1.e5`), a bigint suffix (`1.n`) and a separator
    /// (`1._5`) belong to the literal and are checked as part of it. An `n` or `_`
    /// that starts a longer name (`1.name`, `1._x`) does not.
    fn literal_continues_after_point(&self) -> bool {
        match self.peek_at(1) {
            Some(b'e' | b'E') => true,
            Some(b'n') => !self.identifier_continues_at(2),
            Some(b'_') => {
                let mut offset = 1;
                while self.peek_at(offset) == Some(b'_') {
                    offset += 1;
                }
                self.digit_at(offset) || !self.identifier_continues_at(offset)
            }
            _ => false,
        }
    }

    /// Reports `1.toString()`. The `.` is then left as a member access, so the rest
    /// parses as written instead of cascading into further errors.
    fn report_name_after_decimal_point(&mut self, start: u32, int_part: &str) {
        let help = match self.identifier_text_at(1) {
            Some(name) if is_plain_decimal_integer(int_part) => format!(
                "wrap the number in parentheses, `({int_part}).{name}`, or write `{int_part}..{name}`"
            ),
            _ => "wrap the number in parentheses, or write a second `.`".to_string(),
        };
        self.error_with_help(
            self.span(start, self.pos + 1),
            format!("`{int_part}.` is a complete number, so a name cannot follow it directly"),
            vec![help],
        );
    }

    /// Strict-mode JavaScript and TypeScript reject an integer part that starts with
    /// `0` and has more digits: legacy octal (`010`, which sloppy JavaScript reads as
    /// 8), decimals with a leading zero (`09`, `08.5`), and a separator after the
    /// zero (`0_1`).
    fn reject_leading_zero(&mut self, span: Span) {
        // An invalid span is reported as a fatal error by `next_token`.
        let Ok(literal) = span.text(self.source, self.file) else {
            return;
        };
        let int_len = literal
            .bytes()
            .take_while(|b| b.is_ascii_digit() || *b == b'_')
            .count();
        let (int_part, rest) = literal.split_at(int_len);
        let digits = without_separators(int_part);
        if digits.len() < 2 || !digits.starts_with('0') {
            return;
        }
        let significant = int_part.trim_start_matches(['0', '_']);
        let suggested_literal = if significant.is_empty() {
            format!("0{rest}")
        } else {
            format!("{significant}{rest}")
        };
        if int_part.as_bytes().get(1) == Some(&b'_') {
            self.error_with_help(
                span,
                format!("a numeric separator cannot follow a leading `0` in `{literal}`"),
                vec![format!("write `{suggested_literal}`")],
            );
            return;
        }
        if matches!(rest, "" | "n") && is_legacy_octal_digits(&digits) {
            let help = if significant.is_empty() {
                format!("write `{suggested_literal}`")
            } else {
                format!(
                    "write `0o{significant}{rest}` for octal, or `{suggested_literal}` for decimal"
                )
            };
            self.error_with_help(
                span,
                format!("legacy octal literal `{literal}` is not allowed"),
                vec![help],
            );
            return;
        }
        self.error_with_help(
            span,
            format!("decimal literal `{literal}` cannot have a leading zero"),
            vec![format!("write `{suggested_literal}`")],
        );
    }

    /// Lex a radix-prefixed integer literal (`0x`/`0b`/`0o`). `start` points at the
    /// leading `0`; the prefix letter has not been consumed yet. Trailing junk (e.g.
    /// `0xfg`) is left for the next token, matching how `123abc` lexes as `123` + `abc`.
    fn lex_radix_number(&mut self, start: u32, radix: Radix) -> Token {
        self.pos += 2; // `0` + prefix letter
        let digits_start = self.pos;
        self.scan_digits(|b| radix.accepts(b));
        let Some(digits) = self.source.get(digits_start as usize..self.pos as usize) else {
            return self.fail("invalid numeric literal span");
        };
        let digits = without_separators(digits);
        if digits.is_empty() {
            let span = self.span(start, self.pos);
            self.error(span, format!("missing digits after `{}`", radix.prefix()));
            return Token::new(TokenKind::NumberLiteral(f64::NAN), span);
        }

        if self.peek() == Some(b'n') {
            self.pos += 1;
            return Token::new(
                TokenKind::BigIntLiteral(radix.to_decimal(&digits)),
                self.span(start, self.pos),
            );
        }

        Token::new(
            TokenKind::NumberLiteral(radix.to_f64(&digits)),
            self.span(start, self.pos),
        )
    }

    /// Consumes a run of digits that may contain numeric separators (`1_000`). A
    /// separator is valid only as a single `_` between two digits of the run; a
    /// misplaced one is reported and consumed, and a run that doesn't continue with
    /// a digit after it ends there.
    fn scan_digits(&mut self, is_digit: impl Fn(u8) -> bool) {
        let mut follows_digit = false;
        loop {
            match self.peek() {
                Some(b) if is_digit(b) => {
                    self.pos += 1;
                    follows_digit = true;
                }
                Some(b'_') => {
                    let separator_start = self.pos;
                    while self.peek() == Some(b'_') {
                        self.pos += 1;
                    }
                    let precedes_digit = self.peek().is_some_and(&is_digit);
                    self.check_separator(separator_start, follows_digit && precedes_digit);
                    if !precedes_digit {
                        return;
                    }
                    follows_digit = false;
                }
                _ => return,
            }
        }
    }

    fn check_separator(&mut self, start: u32, is_between_digits: bool) {
        let span = self.span(start, self.pos);
        if !is_between_digits {
            self.error_with_help(
                span,
                "numeric separators are only allowed between digits",
                vec!["remove the `_`".to_string()],
            );
        } else if self.pos - start > 1 {
            self.error_with_help(
                span,
                "only one numeric separator is allowed between digits",
                vec!["remove the extra `_`".to_string()],
            );
        }
    }

    fn identifier_starts_at(&self, offset: usize) -> bool {
        match self.peek_at(offset) {
            Some(b) if b.is_ascii_alphabetic() || b == b'$' || b == b'_' => true,
            Some(0x80..) => self.char_at(offset).is_some_and(is_xid_start),
            _ => false,
        }
    }

    fn identifier_continues_at(&self, offset: usize) -> bool {
        self.char_at(offset).is_some_and(is_identifier_continue)
    }

    /// The identifier starting `offset` bytes ahead, for a help message.
    fn identifier_text_at(&self, offset: usize) -> Option<&'a str> {
        let start = (self.pos as usize).checked_add(offset)?;
        let rest = self.source.get(start..)?;
        let len: usize = rest
            .chars()
            .take_while(|&c| is_identifier_continue(c))
            .map(char::len_utf8)
            .sum();
        rest.get(..len).filter(|name| !name.is_empty())
    }

    fn char_at(&self, offset: usize) -> Option<char> {
        let start = (self.pos as usize).checked_add(offset)?;
        self.source.get(start..)?.chars().next()
    }

    fn lex_string(&mut self, quote: u8) -> Token {
        let start = self.pos;
        self.pos += 1; // consume opening quote
        let mut value = String::new();

        loop {
            match self.peek() {
                None => {
                    self.error(self.span(start, self.pos), "unterminated string literal");
                    return Token::new(TokenKind::StringLiteral(value), self.span(start, self.pos));
                }
                Some(b) if b == quote => {
                    self.pos += 1;
                    return Token::new(TokenKind::StringLiteral(value), self.span(start, self.pos));
                }
                Some(b'\\') => self.read_escape(&mut value),
                // Raw newlines are accepted as `\n` (forgiveness principle): LLMs
                // routinely emit them via tool-input escaping slips, and the intent
                // is unambiguous. CR/CRLF normalise to LF as in templates.
                Some(b'\r') => {
                    self.pos += 1;
                    if self.peek() == Some(b'\n') {
                        self.pos += 1;
                    }
                    value.push('\n');
                }
                Some(_) => {
                    let Some(c) = self.peek_char() else {
                        return self.fail("lexer cursor is not at a source character");
                    };
                    value.push(c);
                    self.pos += c.len_utf8() as u32;
                }
            }
        }
    }

    /// Lex a regex literal. Escape sequences are preserved verbatim (the regex engine
    /// interprets them). `/` inside `[...]` does not close the literal.
    fn lex_regex_literal(&mut self) -> Token {
        let start = self.pos;
        self.pos += 1; // consume opening `/`
        let mut source = String::new();
        let mut in_class = false;

        loop {
            match self.peek() {
                None | Some(b'\n' | b'\r') => {
                    self.error(self.span(start, self.pos), "unterminated regex literal");
                    return Token::new(
                        TokenKind::RegexLiteral {
                            source,
                            flags: String::new(),
                        },
                        self.span(start, self.pos),
                    );
                }
                Some(b'\\') => {
                    source.push('\\');
                    self.pos += 1;
                    if let Some(c) = self.peek_char() {
                        // JS disallows newline-continued escapes in regex literals.
                        if matches!(c, '\n' | '\r') {
                            continue;
                        }
                        source.push(c);
                        self.pos += c.len_utf8() as u32;
                    }
                }
                Some(b'[') if !in_class => {
                    in_class = true;
                    source.push('[');
                    self.pos += 1;
                }
                Some(b']') if in_class => {
                    in_class = false;
                    source.push(']');
                    self.pos += 1;
                }
                Some(b'/') if !in_class => {
                    self.pos += 1;
                    let flags = self.lex_regex_flags();
                    return Token::new(
                        TokenKind::RegexLiteral { source, flags },
                        self.span(start, self.pos),
                    );
                }
                Some(_) => {
                    let Some(c) = self.peek_char() else {
                        return self.fail("lexer cursor is not at a source character");
                    };
                    source.push(c);
                    self.pos += c.len_utf8() as u32;
                }
            }
        }
    }

    /// Accepts any ASCII alphabetic letter; flag validation (`gimsuy` only) is the translator's job.
    fn lex_regex_flags(&mut self) -> String {
        let mut flags = String::new();
        while let Some(b) = self.peek() {
            if b.is_ascii_alphabetic() {
                flags.push(b as char);
                self.pos += 1;
            } else {
                break;
            }
        }
        flags
    }

    fn read_escape(&mut self, out: &mut String) {
        let esc_start = self.pos;
        self.pos += 1;
        match self.peek() {
            None => {
                self.error(
                    self.span(esc_start, self.pos),
                    "unterminated string literal",
                );
            }
            Some(b'"') => {
                out.push('"');
                self.pos += 1;
            }
            Some(b'\'') => {
                out.push('\'');
                self.pos += 1;
            }
            Some(b'\\') => {
                out.push('\\');
                self.pos += 1;
            }
            // JSON's escape set includes `\/`, so strings pasted from JSON carry it.
            Some(b'/') => {
                out.push('/');
                self.pos += 1;
            }
            Some(b'n') => {
                out.push('\n');
                self.pos += 1;
            }
            Some(b't') => {
                out.push('\t');
                self.pos += 1;
            }
            Some(b'r') => {
                out.push('\r');
                self.pos += 1;
            }
            Some(b'b') => {
                out.push('\u{08}');
                self.pos += 1;
            }
            Some(b'f') => {
                out.push('\u{0C}');
                self.pos += 1;
            }
            Some(b'v') => {
                out.push('\u{0B}');
                self.pos += 1;
            }
            Some(b'0') => {
                out.push('\0');
                self.pos += 1;
            }
            Some(b'u') => {
                self.pos += 1;
                self.read_unicode_escape(esc_start, out);
            }
            Some(_) => {
                let Some(c) = self.peek_char() else {
                    self.fail("escape cursor is not at a source character");
                    return;
                };
                let end = self.pos + c.len_utf8() as u32;
                self.error_with_help(
                    self.span(esc_start, end),
                    format!("unknown escape sequence `\\{c}`"),
                    vec![VALID_ESCAPES.to_string()],
                );
                out.push(c);
                self.pos = end;
            }
        }
    }

    fn read_unicode_escape(&mut self, esc_start: u32, out: &mut String) {
        if self.peek() == Some(b'{') {
            self.pos += 1;
            let hex_start = self.pos;
            while matches!(self.peek(), Some(b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F')) {
                self.pos += 1;
            }
            let hex_end = self.pos;
            if hex_end == hex_start {
                self.error(
                    self.span(esc_start, self.pos),
                    "invalid unicode escape: expected hex digits",
                );
                return;
            }
            if self.peek() != Some(b'}') {
                self.error(
                    self.span(esc_start, self.pos),
                    "invalid unicode escape: expected `}`",
                );
                return;
            }
            self.pos += 1;
            let Some(hex) = self.source.get(hex_start as usize..hex_end as usize) else {
                self.fail("invalid Unicode escape span");
                return;
            };
            if hex.len() > 6 {
                self.error(
                    self.span(esc_start, self.pos),
                    "invalid code point in `\\u{…}`: too many digits",
                );
                return;
            }
            let value = u32::from_str_radix(hex, 16).unwrap_or(0);
            if value > 0x10FFFF {
                self.error(
                    self.span(esc_start, self.pos),
                    "invalid code point in `\\u{…}`: exceeds U+10FFFF",
                );
                return;
            }
            if (0xD800..=0xDFFF).contains(&value) {
                self.error(
                    self.span(esc_start, self.pos),
                    "invalid code point in `\\u{…}`: surrogate",
                );
                return;
            }
            if let Some(c) = char::from_u32(value) {
                out.push(c);
            }
        } else {
            let Some(value) = self.read_four_hex(esc_start) else {
                return;
            };
            if (0xD800..=0xDBFF).contains(&value) {
                // High surrogate — try to read a following \uYYYY low-surrogate.
                let save = self.pos;
                if self.peek() == Some(b'\\')
                    && self.peek_at(1) == Some(b'u')
                    && self.peek_at(2) != Some(b'{')
                {
                    self.pos += 2; // consume `\u`
                    if let Some(low) = self.read_four_hex(save)
                        && (0xDC00..=0xDFFF).contains(&low)
                    {
                        let code = 0x10000 + ((value - 0xD800) << 10) + (low - 0xDC00);
                        if let Some(c) = char::from_u32(code) {
                            out.push(c);
                        }
                        return;
                    }
                    self.pos = save;
                }
                self.error(self.span(esc_start, self.pos), "lone high surrogate");
            } else if (0xDC00..=0xDFFF).contains(&value) {
                self.error(self.span(esc_start, self.pos), "lone low surrogate");
            } else if let Some(c) = char::from_u32(value) {
                out.push(c);
            }
        }
    }

    fn read_four_hex(&mut self, esc_start: u32) -> Option<u32> {
        let mut value = 0u32;
        for _ in 0..4 {
            if let Some(d) = self.peek().and_then(|byte| char::from(byte).to_digit(16)) {
                value = value * 16 + d;
                self.pos += 1;
            } else {
                self.error(
                    self.span(esc_start, self.pos),
                    "invalid unicode escape: expected 4 hex digits",
                );
                return None;
            }
        }
        Some(value)
    }

    fn lex_ident(&mut self) -> Token {
        let start = self.pos;
        let Some(first) = self.peek_char() else {
            return self.fail("identifier lexer has no source character");
        };
        self.pos += first.len_utf8() as u32;
        while let Some(c) = self.peek_char() {
            if is_identifier_continue(c) {
                self.pos += c.len_utf8() as u32;
            } else {
                break;
            }
        }
        let span = self.span(start, self.pos);
        let Some(lexeme) = self.source.get(start as usize..self.pos as usize) else {
            return self.fail("invalid token source span");
        };
        let kind = match lexeme {
            "true" => TokenKind::BooleanLiteral(true),
            "false" => TokenKind::BooleanLiteral(false),
            "null" => TokenKind::NullLiteral,
            "let" => TokenKind::Let,
            "const" => TokenKind::Const,
            "function" => TokenKind::Function,
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "while" => TokenKind::While,
            "do" => TokenKind::Do,
            "for" => TokenKind::For,
            "break" => TokenKind::Break,
            "continue" => TokenKind::Continue,
            "return" => TokenKind::Return,
            "switch" => TokenKind::Switch,
            "case" => TokenKind::Case,
            "default" => TokenKind::Default,
            "export" => TokenKind::Export,
            "void" => TokenKind::Void,
            "interface" => TokenKind::Interface,
            "enum" => TokenKind::Enum,
            "in" => TokenKind::In,
            "typeof" => TokenKind::Typeof,
            "import" => TokenKind::Import,
            "instanceof" => TokenKind::Instanceof,
            "new" => TokenKind::New,
            "try" => TokenKind::Try,
            "catch" => TokenKind::Catch,
            "finally" => TokenKind::Finally,
            "throw" => TokenKind::Throw,
            "class" => TokenKind::Class,
            "extends" => TokenKind::Extends,
            "implements" => TokenKind::Implements,
            "super" => TokenKind::Super,
            "this" => TokenKind::This,
            _ => TokenKind::Identifier,
        };
        Token::new(kind, span)
    }

    // next_token_inner matches the current byte immediately before dispatch.
    fn lex_operator(&mut self) -> Token {
        let start = self.pos;
        let b = self.peek().expect("dispatch matched a source byte");
        let kind = match b {
            b'=' => {
                self.pos += 1;
                if self.peek() == Some(b'=') {
                    self.pos += 1;
                    if self.peek() == Some(b'=') {
                        self.pos += 1;
                        TokenKind::EqEqEq
                    } else {
                        TokenKind::EqEq
                    }
                } else if self.peek() == Some(b'>') {
                    self.pos += 1;
                    TokenKind::Arrow
                } else {
                    TokenKind::Equals
                }
            }
            b'!' => {
                self.pos += 1;
                if self.peek() == Some(b'=') {
                    self.pos += 1;
                    if self.peek() == Some(b'=') {
                        self.pos += 1;
                        TokenKind::BangEqEq
                    } else {
                        TokenKind::BangEq
                    }
                } else {
                    TokenKind::Bang
                }
            }
            b'<' => {
                self.pos += 1;
                if self.peek() == Some(b'=') {
                    self.pos += 1;
                    TokenKind::LessEquals
                } else {
                    TokenKind::LessThan
                }
            }
            b'>' => {
                self.pos += 1;
                if self.peek() == Some(b'=') {
                    self.pos += 1;
                    TokenKind::GreaterEquals
                } else {
                    TokenKind::GreaterThan
                }
            }
            b'+' => {
                self.pos += 1;
                match self.peek() {
                    Some(b'+') => {
                        self.pos += 1;
                        TokenKind::PlusPlus
                    }
                    Some(b'=') => {
                        self.pos += 1;
                        TokenKind::PlusEquals
                    }
                    _ => TokenKind::Plus,
                }
            }
            b'-' => {
                self.pos += 1;
                match self.peek() {
                    Some(b'-') => {
                        self.pos += 1;
                        TokenKind::MinusMinus
                    }
                    Some(b'=') => {
                        self.pos += 1;
                        TokenKind::MinusEquals
                    }
                    _ => TokenKind::Minus,
                }
            }
            b'*' => {
                self.pos += 1;
                match self.peek() {
                    Some(b'*') => {
                        self.pos += 1;
                        // `**=` checked before `**` so `x **= 2` doesn't lex as `**` + `=`.
                        if self.peek() == Some(b'=') {
                            self.pos += 1;
                            TokenKind::StarStarEquals
                        } else {
                            TokenKind::StarStar
                        }
                    }
                    Some(b'=') => {
                        self.pos += 1;
                        TokenKind::StarEquals
                    }
                    _ => TokenKind::Star,
                }
            }
            b'/' => {
                // `//` and `/*` already consumed as trivia; remaining `/` is regex or division.
                if self.regex_context() {
                    return self.lex_regex_literal();
                }
                self.pos += 1;
                if self.peek() == Some(b'=') {
                    self.pos += 1;
                    TokenKind::SlashEquals
                } else {
                    TokenKind::Slash
                }
            }
            b'%' => {
                self.pos += 1;
                if self.peek() == Some(b'=') {
                    self.pos += 1;
                    TokenKind::PercentEquals
                } else {
                    TokenKind::Percent
                }
            }
            b'&' => {
                self.pos += 2;
                TokenKind::AmpAmp
            }
            b'|' => {
                self.pos += 1;
                if self.peek() == Some(b'|') {
                    self.pos += 1;
                    TokenKind::PipePipe
                } else {
                    TokenKind::Pipe
                }
            }
            b'.' => {
                self.pos += 1;
                // Single token to avoid 3-token lookahead in the parser.
                if self.peek() == Some(b'.') && self.peek_at(1) == Some(b'.') {
                    self.pos += 2;
                    TokenKind::DotDotDot
                } else {
                    TokenKind::Dot
                }
            }
            _ => unreachable!("operator dispatch requires an operator byte"),
        };
        Token::new(kind, self.span(start, self.pos))
    }

    // next_token_inner matches the current byte immediately before dispatch.
    fn lex_delimiter(&mut self) -> Token {
        let start = self.pos;
        let b = self.peek().expect("dispatch matched a source byte");
        if let Some(depth) = self.template_frames.last_mut() {
            if b == b'{' {
                let Some(next) = depth.checked_add(1) else {
                    return self.fail("template brace depth overflow");
                };
                *depth = next;
            } else if b == b'}' {
                if *depth == 0 {
                    self.template_frames.pop();
                    self.pos += 1;
                    return self.lex_template_part(start, false);
                }
                *depth -= 1;
            }
        }
        self.pos += 1;
        let kind = match b {
            b'(' => TokenKind::LeftParen,
            b')' => TokenKind::RightParen,
            b'{' => TokenKind::LeftBrace,
            b'}' => TokenKind::RightBrace,
            b'[' => TokenKind::LeftBracket,
            b']' => TokenKind::RightBracket,
            b',' => TokenKind::Comma,
            b':' => TokenKind::Colon,
            b';' => TokenKind::Semicolon,
            b'?' => match self.peek() {
                // `?.5` is a ternary on `.5`, as in JavaScript, not optional chaining.
                Some(b'.') if !self.digit_at(1) => {
                    self.pos += 1;
                    TokenKind::QuestionDot
                }
                Some(b'?') => {
                    self.pos += 1;
                    TokenKind::QuestionQuestion
                }
                _ => TokenKind::Question,
            },
            _ => unreachable!("delimiter dispatch requires a delimiter byte"),
        };
        Token::new(kind, self.span(start, self.pos))
    }

    /// Scan one cooked segment of a template literal.
    /// `start` is the offset of the leading delimiter; `is_head` selects
    /// Head/NoSubstitution (true) vs. Middle/Tail (false).
    fn lex_template_part(&mut self, start: u32, is_head: bool) -> Token {
        let mut value = String::new();
        loop {
            match self.peek() {
                None => {
                    self.error(self.span(start, self.pos), "unterminated template literal");
                    let kind = if is_head {
                        TokenKind::TemplateNoSubstitution(value)
                    } else {
                        TokenKind::TemplateTail(value)
                    };
                    return Token::new(kind, self.span(start, self.pos));
                }
                Some(b'`') => {
                    self.pos += 1;
                    let kind = if is_head {
                        TokenKind::TemplateNoSubstitution(value)
                    } else {
                        TokenKind::TemplateTail(value)
                    };
                    return Token::new(kind, self.span(start, self.pos));
                }
                Some(b'$') if self.peek_at(1) == Some(b'{') => {
                    self.pos += 2; // consume `${`
                    self.template_frames.push(0);
                    let kind = if is_head {
                        TokenKind::TemplateHead(value)
                    } else {
                        TokenKind::TemplateMiddle(value)
                    };
                    return Token::new(kind, self.span(start, self.pos));
                }
                Some(b'\\') => self.read_template_escape(&mut value),
                Some(b'\r') => {
                    // Normalise CR/CRLF to LF so platform line endings don't affect the cooked value.
                    self.pos += 1;
                    if self.peek() == Some(b'\n') {
                        self.pos += 1;
                    }
                    value.push('\n');
                }
                Some(_) => {
                    let Some(c) = self.peek_char() else {
                        return self.fail("lexer cursor is not at a source character");
                    };
                    value.push(c);
                    self.pos += c.len_utf8() as u32;
                }
            }
        }
    }

    /// Like `read_escape` but also handles `` \` `` and `\$` (template-only escapes).
    fn read_template_escape(&mut self, out: &mut String) {
        match self.peek_at(1) {
            Some(b'`') => {
                out.push('`');
                self.pos += 2;
            }
            Some(b'$') => {
                out.push('$');
                self.pos += 2;
            }
            Some(_) => self.read_escape(out),
            None => self.pos += 1,
        }
    }
}

fn is_identifier_continue(c: char) -> bool {
    c == '_' || c == '$' || is_xid_continue(c)
}

/// What the `.` right after a decimal integer part is.
enum DecimalPoint {
    /// Part of the literal: `1.`, `1.5`, `1.e5`, and the first `.` of `1..toString()`.
    Literal,
    /// A member access after a legacy octal integer (`010.toString()`), which has no
    /// fraction in JavaScript.
    MemberAccess,
    /// A name directly after the `.` (`1.toString()`), which is an error.
    NameAfter,
}

/// Whether `int_part` is a well-formed decimal integer, so a help message can repeat it.
fn is_plain_decimal_integer(int_part: &str) -> bool {
    let has_leading_zero = int_part.len() > 1 && int_part.starts_with('0');
    !has_leading_zero && !int_part.ends_with('_') && !int_part.contains("__")
}

/// An integer part sloppy JavaScript reads as octal: a `0` followed by octal digits.
fn is_legacy_octal_digits(int_part: &str) -> bool {
    int_part.len() >= 2
        && int_part.starts_with('0')
        && int_part.bytes().all(|digit| matches!(digit, b'0'..=b'7'))
}

fn without_separators(digits: &str) -> String {
    digits.replace('_', "")
}

/// A radix-prefixed integer literal base (`0x`, `0b`, `0o`).
#[derive(Clone, Copy)]
enum Radix {
    Hex,
    Binary,
    Octal,
}

impl Radix {
    fn from_prefix(b: u8) -> Option<Self> {
        match b {
            b'x' | b'X' => Some(Self::Hex),
            b'b' | b'B' => Some(Self::Binary),
            b'o' | b'O' => Some(Self::Octal),
            _ => None,
        }
    }

    fn accepts(self, b: u8) -> bool {
        match self {
            Self::Hex => b.is_ascii_hexdigit(),
            Self::Binary => matches!(b, b'0' | b'1'),
            Self::Octal => matches!(b, b'0'..=b'7'),
        }
    }

    fn base(self) -> u32 {
        match self {
            Self::Hex => 16,
            Self::Binary => 2,
            Self::Octal => 8,
        }
    }

    fn prefix(self) -> &'static str {
        match self {
            Self::Hex => "0x",
            Self::Binary => "0b",
            Self::Octal => "0o",
        }
    }

    /// Accumulates in `f64` so an over-wide literal saturates to infinity rather than
    /// panicking; `accepts` already guaranteed every byte is a valid digit.
    fn to_f64(self, digits: &str) -> f64 {
        let base = f64::from(self.base());
        digits
            .bytes()
            .map(|b| f64::from(hex_digit_value(b)))
            .fold(0.0, |acc, d| acc * base + d)
    }

    fn to_decimal(self, digits: &str) -> String {
        BigUint::from_str_radix(digits, self.base())
            .map_or_else(|_| "0".to_string(), |v| v.to_str_radix(10))
    }
}

/// Unicode space separators (category Zs) beyond ASCII space, which JavaScript treats
/// as whitespace. A no-break space pasted from a web page or word processor is the
/// usual source.
fn is_space_separator(c: char) -> bool {
    matches!(
        c,
        '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}'
    )
}

/// Numeric value of an ASCII hex digit; non-digits (never passed by `Radix::accepts`)
/// map to `0`.
fn hex_digit_value(b: u8) -> u32 {
    match b {
        b'0'..=b'9' => u32::from(b - b'0'),
        b'a'..=b'f' => u32::from(b - b'a' + 10),
        b'A'..=b'F' => u32::from(b - b'A' + 10),
        _ => 0,
    }
}

/// Returns `true` when `/` should start a regex literal rather than act as division.
/// `None` (start of input) is treated as regex-context so `/foo/` at program start works.
#[allow(clippy::match_like_matches_macro)] // grouped arms read clearer than a flat `matches!`
pub fn is_regex_context(prev: &Option<TokenKind>) -> bool {
    let Some(kind) = prev else {
        return true;
    };
    match kind {
        TokenKind::Plus
        | TokenKind::Minus
        | TokenKind::Star
        | TokenKind::Slash
        | TokenKind::Percent
        | TokenKind::Equals
        | TokenKind::PlusEquals
        | TokenKind::MinusEquals
        | TokenKind::StarEquals
        | TokenKind::SlashEquals
        | TokenKind::PercentEquals
        | TokenKind::StarStar
        | TokenKind::StarStarEquals
        | TokenKind::EqEqEq
        | TokenKind::EqEq
        | TokenKind::BangEqEq
        | TokenKind::BangEq
        | TokenKind::LessThan
        | TokenKind::GreaterThan
        | TokenKind::LessEquals
        | TokenKind::GreaterEquals
        | TokenKind::Bang
        | TokenKind::AmpAmp
        | TokenKind::Pipe
        | TokenKind::PipePipe
        | TokenKind::Question
        | TokenKind::QuestionDot
        | TokenKind::QuestionQuestion
        | TokenKind::Arrow
        | TokenKind::DotDotDot => true,

        TokenKind::LeftParen
        | TokenKind::LeftBrace
        | TokenKind::LeftBracket
        | TokenKind::Comma
        | TokenKind::Colon
        | TokenKind::Semicolon => true,

        TokenKind::Return
        | TokenKind::Throw
        | TokenKind::Typeof
        | TokenKind::In
        | TokenKind::New
        | TokenKind::Case
        | TokenKind::Default
        | TokenKind::Else
        | TokenKind::Do
        | TokenKind::Void => true,

        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::Lexer;
    use crate::source::Sources;
    use crate::{Diagnostic, FileId, Span, Token, TokenKind, diagnostics};

    const F: FileId = FileId(0);

    #[test]
    fn invalid_unicode_cursor_and_template_depth_return_fatal_errors() {
        let mut lexer = Lexer::new("é", F);
        lexer.pos = 1;
        assert!(matches!(lexer.lex_ident().kind, TokenKind::Eof));
        assert!(
            lexer
                .finish()
                .expect_err("invalid UTF-8 cursor")
                .fatal
                .is_some()
        );

        let mut lexer = Lexer::new("{", F);
        lexer.template_frames.push(u32::MAX);
        assert!(matches!(lexer.lex_delimiter().kind, TokenKind::Eof));
        assert!(
            lexer
                .finish()
                .expect_err("template depth overflow")
                .fatal
                .is_some()
        );
    }

    fn sources(text: &str) -> Sources {
        let (sources, _) = Sources::single("script.subm", text).unwrap();
        sources
    }

    fn tokenize_all(source: &str) -> (Vec<Token>, Vec<Diagnostic>) {
        let mut lx = Lexer::new(source, crate::FileId(0));
        let mut tokens = Vec::new();
        loop {
            let tok = lx.next_token();
            let is_eof = tok.kind == TokenKind::Eof;
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        (tokens, lx.into_diagnostics())
    }

    fn tokenize_one(source: &str) -> (Token, Token, Vec<Diagnostic>) {
        let mut lx = Lexer::new(source, crate::FileId(0));
        let first = lx.next_token();
        let second = lx.next_token();
        (first, second, lx.into_diagnostics())
    }

    fn expect_number(source: &str, value: f64, span: Span) {
        let (tok, eof, diags) = tokenize_one(source);
        assert_eq!(tok.span, span, "span mismatch for {source:?}");
        match tok.kind {
            TokenKind::NumberLiteral(v) => assert_eq!(v, value, "value mismatch for {source:?}"),
            other => panic!("expected NumberLiteral for {source:?}, got {other:?}"),
        }
        assert_eq!(eof.kind, TokenKind::Eof);
        assert!(
            diags.is_empty(),
            "unexpected diagnostics for {source:?}: {diags:?}"
        );
    }

    fn expect_string(source: &str, value: &str) {
        let (tok, eof, diags) = tokenize_one(source);
        match tok.kind {
            TokenKind::StringLiteral(ref s) => {
                assert_eq!(s, value, "string mismatch for {source:?}");
            }
            other => panic!("expected StringLiteral for {source:?}, got {other:?}"),
        }
        assert_eq!(eof.kind, TokenKind::Eof);
        assert!(
            diags.is_empty(),
            "unexpected diagnostics for {source:?}: {diags:?}"
        );
    }

    fn expect_single_token(source: &str, expected: TokenKind, span: Span) {
        let (tok, eof, diags) = tokenize_one(source);
        assert_eq!(tok.kind, expected, "kind mismatch for {source:?}");
        assert_eq!(tok.span, span, "span mismatch for {source:?}");
        assert_eq!(eof.kind, TokenKind::Eof);
        assert!(
            diags.is_empty(),
            "unexpected diagnostics for {source:?}: {diags:?}"
        );
    }

    #[test]
    fn slash_after_non_null_assertion_is_division() {
        for prefix in ["a!", "f()!", "a[0]!", "o?.b!.count!", "a!!"] {
            for (operator, expected) in [("/", TokenKind::Slash), ("/=", TokenKind::SlashEquals)] {
                let (tokens, diagnostics) = tokenize_all(&format!("{prefix} {operator} 10"));
                assert!(
                    diagnostics.is_empty(),
                    "{prefix} {operator}: {diagnostics:?}"
                );
                assert!(tokens.iter().any(|token| token.kind == expected));
            }
        }
        for source in ["!/x/.test(s)", "!!/x/.test(s)", "a! / /x/.test(s)"] {
            let (tokens, diagnostics) = tokenize_all(source);
            assert!(diagnostics.is_empty(), "{source}: {diagnostics:?}");
            assert!(
                tokens
                    .iter()
                    .any(|token| matches!(token.kind, TokenKind::RegexLiteral { .. }))
            );
        }
    }

    #[test]
    fn lex_integer() {
        expect_number("42", 42.0, Span::new(F, 0, 2).unwrap());
    }

    #[test]
    fn lex_zero() {
        expect_number("0", 0.0, Span::new(F, 0, 1).unwrap());
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn lex_decimal() {
        expect_number("3.14", 3.14, Span::new(F, 0, 4).unwrap());
    }

    #[test]
    fn lex_leading_zero_decimal() {
        expect_number("0.5", 0.5, Span::new(F, 0, 3).unwrap());
    }

    #[test]
    fn lex_exponent() {
        expect_number("1e10", 1e10, Span::new(F, 0, 4).unwrap());
    }

    #[test]
    fn lex_decimal_with_exponent() {
        expect_number("1.5e10", 1.5e10, Span::new(F, 0, 6).unwrap());
    }

    #[test]
    fn lex_uppercase_exponent_with_negative_sign() {
        expect_number("2E-3", 2e-3, Span::new(F, 0, 4).unwrap());
    }

    #[test]
    fn lex_positive_exponent() {
        expect_number("1e+2", 1e2, Span::new(F, 0, 4).unwrap());
    }

    #[test]
    fn lex_number_with_trailing_whitespace() {
        let mut lx = Lexer::new("42 ", crate::FileId(0));
        let tok = lx.next_token();
        assert_eq!(tok.span, Span::new(F, 0, 2).unwrap());
        assert_eq!(tok.kind, TokenKind::NumberLiteral(42.0));
        assert_eq!(lx.next_token().kind, TokenKind::Eof);
        assert!(lx.into_diagnostics().is_empty());
    }

    #[test]
    fn unicode_space_separators_are_whitespace() {
        let source = "a\u{a0}b\u{1680}c\u{2000}d\u{200a}e\u{202f}f\u{205f}g\u{3000}h\u{0b}i\u{0c}j";
        let (tokens, diags) = tokenize_all(source);
        let names: Vec<_> = tokens
            .iter()
            .filter(|t| t.kind == TokenKind::Identifier)
            .map(|t| &source[t.span.start as usize..t.span.end as usize])
            .collect();
        assert_eq!(names, ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]);
        assert!(diags.is_empty());
    }

    #[test]
    fn zero_width_space_is_not_whitespace() {
        // U+200B is a format character (Cf), not a space separator, in JavaScript too.
        let (_tokens, diags) = tokenize_all("a\u{200b}b");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.starts_with("unexpected character"));
    }

    #[test]
    fn empty_input_is_eof() {
        let mut lx = Lexer::new("", crate::FileId(0));
        let tok = lx.next_token();
        assert_eq!(tok.kind, TokenKind::Eof);
        assert_eq!(tok.span, Span::new(F, 0, 0).unwrap());
        assert!(lx.into_diagnostics().is_empty());
    }

    #[test]
    fn bare_exponent_is_nan_with_diagnostic() {
        let mut lx = Lexer::new("1e", crate::FileId(0));
        let tok = lx.next_token();
        assert_eq!(tok.span, Span::new(F, 0, 2).unwrap());
        match tok.kind {
            TokenKind::NumberLiteral(v) => assert!(v.is_nan()),
            other => panic!("expected NumberLiteral(NaN), got {other:?}"),
        }
        let diags = lx.into_diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].span, Span::new(F, 1, 2).unwrap());
        assert_eq!(diags[0].message, "missing digits in exponent");
    }

    #[test]
    fn signed_exponent_with_no_digits_is_nan_with_diagnostic() {
        let mut lx = Lexer::new("3.14e+", crate::FileId(0));
        let tok = lx.next_token();
        assert_eq!(tok.span, Span::new(F, 0, 6).unwrap());
        match tok.kind {
            TokenKind::NumberLiteral(v) => assert!(v.is_nan()),
            other => panic!("expected NumberLiteral(NaN), got {other:?}"),
        }
        let diags = lx.into_diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].span, Span::new(F, 4, 6).unwrap());
    }

    #[test]
    fn renders_bare_exponent_diagnostic() {
        let source = "1e";
        let mut lx = Lexer::new(source, crate::FileId(0));
        let _ = lx.next_token();
        let diags = lx.into_diagnostics();
        let rendered = diagnostics::render(&diags[0], &sources(source));
        insta::assert_snapshot!(rendered);
    }

    fn expect_bigint(source: &str, digits: &str, span: Span) {
        let (tok, eof, diags) = tokenize_one(source);
        assert_eq!(tok.span, span, "span mismatch for {source:?}");
        match tok.kind {
            TokenKind::BigIntLiteral(ref s) => {
                assert_eq!(s, digits, "digits mismatch for {source:?}");
            }
            other => panic!("expected BigIntLiteral for {source:?}, got {other:?}"),
        }
        assert_eq!(eof.kind, TokenKind::Eof);
        assert!(
            diags.is_empty(),
            "unexpected diagnostics for {source:?}: {diags:?}"
        );
    }

    #[test]
    fn lex_bigint_simple() {
        expect_bigint("42n", "42", Span::new(F, 0, 3).unwrap());
    }

    #[test]
    fn lex_bigint_zero() {
        expect_bigint("0n", "0", Span::new(F, 0, 2).unwrap());
    }

    #[test]
    fn lex_bigint_large_beyond_u64() {
        // Beyond u64::MAX — preserved as raw string for codegen.
        expect_bigint(
            "1267650600228229401496703205376n",
            "1267650600228229401496703205376",
            Span::new(F, 0, 32).unwrap(),
        );
    }

    #[test]
    fn lex_bigint_fraction_rejected() {
        let mut lx = Lexer::new("3.14n", crate::FileId(0));
        let tok = lx.next_token();
        assert_eq!(tok.span, Span::new(F, 0, 5).unwrap());
        match tok.kind {
            TokenKind::BigIntLiteral(ref s) => assert_eq!(s, "3"),
            other => panic!("expected BigIntLiteral (recovery), got {other:?}"),
        }
        let diags = lx.into_diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].span, Span::new(F, 0, 5).unwrap());
        assert!(
            diags[0]
                .message
                .contains("bigint literal cannot have a fractional or exponent part"),
            "unexpected message: {:?}",
            diags[0].message
        );
    }

    #[test]
    fn lex_bigint_exponent_rejected() {
        let mut lx = Lexer::new("1e10n", crate::FileId(0));
        let tok = lx.next_token();
        assert_eq!(tok.span, Span::new(F, 0, 5).unwrap());
        match tok.kind {
            TokenKind::BigIntLiteral(ref s) => assert_eq!(s, "1"),
            other => panic!("expected BigIntLiteral (recovery), got {other:?}"),
        }
        let diags = lx.into_diagnostics();
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0]
                .message
                .contains("bigint literal cannot have a fractional or exponent part"),
            "unexpected message: {:?}",
            diags[0].message
        );
    }

    #[test]
    fn lex_hex_literal() {
        expect_number("0xff", 255.0, Span::new(F, 0, 4).unwrap());
        expect_number("0XFF", 255.0, Span::new(F, 0, 4).unwrap());
        expect_number("0x0", 0.0, Span::new(F, 0, 3).unwrap());
        expect_number("0x10", 16.0, Span::new(F, 0, 4).unwrap());
    }

    #[test]
    fn lex_binary_literal() {
        expect_number("0b1010", 10.0, Span::new(F, 0, 6).unwrap());
        expect_number("0B1", 1.0, Span::new(F, 0, 3).unwrap());
        expect_number("0b0", 0.0, Span::new(F, 0, 3).unwrap());
    }

    #[test]
    fn lex_octal_literal() {
        expect_number("0o17", 15.0, Span::new(F, 0, 4).unwrap());
        expect_number("0O7", 7.0, Span::new(F, 0, 3).unwrap());
        expect_number("0o0", 0.0, Span::new(F, 0, 3).unwrap());
    }

    #[test]
    fn lex_radix_bigint() {
        expect_bigint("0xffn", "255", Span::new(F, 0, 5).unwrap());
        expect_bigint("0b101n", "5", Span::new(F, 0, 6).unwrap());
        expect_bigint("0o17n", "15", Span::new(F, 0, 5).unwrap());
    }

    #[test]
    fn lex_hex_bigint_beyond_u64() {
        expect_bigint(
            "0xffffffffffffffffn",
            "18446744073709551615",
            Span::new(F, 0, 19).unwrap(),
        );
    }

    #[test]
    fn lex_radix_missing_digits() {
        for src in ["0x", "0b", "0o"] {
            let mut lx = Lexer::new(src, crate::FileId(0));
            let tok = lx.next_token();
            assert_eq!(tok.span, Span::new(F, 0, 2).unwrap(), "span for {src:?}");
            assert!(
                matches!(tok.kind, TokenKind::NumberLiteral(v) if v.is_nan()),
                "expected NaN recovery for {src:?}, got {:?}",
                tok.kind
            );
            let diags = lx.into_diagnostics();
            assert_eq!(diags.len(), 1, "diags for {src:?}");
            assert!(
                diags[0].message.contains("missing digits after"),
                "unexpected message for {src:?}: {:?}",
                diags[0].message
            );
        }
    }

    fn expect_single_leading_zero_error(source: &str, message: &str, help: &str) {
        let (_, eof, diags) = tokenize_one(source);
        assert_eq!(
            eof.kind,
            TokenKind::Eof,
            "{source:?} should lex as one token"
        );
        assert_eq!(diags.len(), 1, "diags for {source:?}: {diags:?}");
        assert_eq!(diags[0].message, message, "message for {source:?}");
        assert_eq!(diags[0].help, vec![help.to_string()], "help for {source:?}");
        let source_len = u32::try_from(source.len()).unwrap();
        assert_eq!(diags[0].span, Span::new(F, 0, source_len).unwrap());
    }

    #[test]
    fn lex_legacy_octal_rejected() {
        expect_single_leading_zero_error(
            "010",
            "legacy octal literal `010` is not allowed",
            "write `0o10` for octal, or `10` for decimal",
        );
        expect_single_leading_zero_error(
            "010n",
            "legacy octal literal `010n` is not allowed",
            "write `0o10n` for octal, or `10n` for decimal",
        );
        expect_single_leading_zero_error(
            "000",
            "legacy octal literal `000` is not allowed",
            "write `0`",
        );
    }

    #[test]
    fn lex_leading_zero_decimal_rejected() {
        expect_single_leading_zero_error(
            "09",
            "decimal literal `09` cannot have a leading zero",
            "write `9`",
        );
        expect_single_leading_zero_error(
            "08.5",
            "decimal literal `08.5` cannot have a leading zero",
            "write `8.5`",
        );
        expect_single_leading_zero_error(
            "00.5",
            "decimal literal `00.5` cannot have a leading zero",
            "write `0.5`",
        );
        expect_single_leading_zero_error(
            "07e1",
            "decimal literal `07e1` cannot have a leading zero",
            "write `7e1`",
        );
        expect_single_leading_zero_error(
            "08n",
            "decimal literal `08n` cannot have a leading zero",
            "write `8n`",
        );
    }

    #[test]
    fn lex_single_leading_zero_accepted() {
        expect_number("0", 0.0, Span::new(F, 0, 1).unwrap());
        expect_number("0.5", 0.5, Span::new(F, 0, 3).unwrap());
        expect_number("0e1", 0.0, Span::new(F, 0, 3).unwrap());
        expect_number("0.010", 0.01, Span::new(F, 0, 5).unwrap());
        expect_number("1e010", 1e10, Span::new(F, 0, 5).unwrap());
        expect_number("0x010", 16.0, Span::new(F, 0, 5).unwrap());
        expect_bigint("0n", "0", Span::new(F, 0, 2).unwrap());
    }

    #[test]
    fn lex_numeric_separators() {
        expect_number("1_000", 1000.0, Span::new(F, 0, 5).unwrap());
        expect_number("1_000.5_5", 1000.55, Span::new(F, 0, 9).unwrap());
        expect_number("1e1_0", 1e10, Span::new(F, 0, 5).unwrap());
        expect_number("0.0_1", 0.01, Span::new(F, 0, 5).unwrap());
        expect_number("0xF_F", 255.0, Span::new(F, 0, 5).unwrap());
        expect_number("0b1_0", 2.0, Span::new(F, 0, 5).unwrap());
        expect_number("0o1_7", 15.0, Span::new(F, 0, 5).unwrap());
        expect_bigint("1_000n", "1000", Span::new(F, 0, 6).unwrap());
        expect_bigint("0xF_Fn", "255", Span::new(F, 0, 6).unwrap());
        expect_number("0XF_F", 255.0, Span::new(F, 0, 5).unwrap());
        expect_number("0B1_0", 2.0, Span::new(F, 0, 5).unwrap());
        expect_number("0O1_7", 15.0, Span::new(F, 0, 5).unwrap());
        expect_number("1E1_0", 1e10, Span::new(F, 0, 5).unwrap());
        expect_number("1e+1_0", 1e10, Span::new(F, 0, 6).unwrap());
        expect_number("1e-1_0", 1e-10, Span::new(F, 0, 6).unwrap());
    }

    #[test]
    fn lex_misplaced_numeric_separator_rejected() {
        for source in [
            "1_", "1_.5", "1._5", "1_e5", "1e_5", "1e+_5", "1_n", "0x_1", "0x1_",
        ] {
            let (_, eof, diags) = tokenize_one(source);
            assert_eq!(
                eof.kind,
                TokenKind::Eof,
                "{source:?} should lex as one token"
            );
            assert_eq!(diags.len(), 1, "diags for {source:?}: {diags:?}");
            assert_eq!(
                diags[0].message, "numeric separators are only allowed between digits",
                "message for {source:?}"
            );
        }
        let (_, _, diags) = tokenize_one("1__0");
        assert_eq!(diags.len(), 1, "diags: {diags:?}");
        assert_eq!(
            diags[0].message,
            "only one numeric separator is allowed between digits"
        );
        assert_eq!(diags[0].span, Span::new(F, 1, 3).unwrap());
        expect_single_leading_zero_error(
            "0_1",
            "a numeric separator cannot follow a leading `0` in `0_1`",
            "write `1`",
        );
    }

    #[test]
    fn lex_number_ending_in_decimal_point() {
        expect_number("1.", 1.0, Span::new(F, 0, 2).unwrap());
        expect_number("1.e5", 1e5, Span::new(F, 0, 4).unwrap());
        expect_number("0.", 0.0, Span::new(F, 0, 2).unwrap());
        let (tokens, diags) = tokenize_all("1..toString");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert!(matches!(tokens[0].kind, TokenKind::NumberLiteral(v) if v == 1.0));
        assert_eq!(tokens[0].span, Span::new(F, 0, 2).unwrap());
        assert_eq!(tokens[1].kind, TokenKind::Dot);
    }

    #[test]
    fn lex_number_starting_with_decimal_point() {
        expect_number(".5", 0.5, Span::new(F, 0, 2).unwrap());
        expect_number(".5e1", 5.0, Span::new(F, 0, 4).unwrap());
        expect_number(".5_5", 0.55, Span::new(F, 0, 4).unwrap());
        // `?.` before a digit is a ternary `?` and a number, as in JavaScript.
        let (tokens, diags) = tokenize_all("c?.5:1");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(tokens[1].kind, TokenKind::Question);
        assert!(matches!(tokens[2].kind, TokenKind::NumberLiteral(v) if v == 0.5));
        let (tokens, _) = tokenize_all("c?.x");
        assert_eq!(tokens[1].kind, TokenKind::QuestionDot);
        // Errors in a leading-dot literal are checked as in any other.
        let (_, _, diags) = tokenize_one(".5n");
        assert_eq!(diags.len(), 1, "diags: {diags:?}");
        assert!(
            diags[0]
                .message
                .contains("bigint literal cannot have a fractional")
        );
        let (_, _, diags) = tokenize_one(".5_");
        assert_eq!(diags.len(), 1, "diags: {diags:?}");
        assert_eq!(
            diags[0].message,
            "numeric separators are only allowed between digits"
        );
    }

    #[test]
    fn lex_name_after_decimal_point_rejected() {
        // A name starting with `n` or `_` is a name, not a bigint suffix or separator.
        for (source, literal_end) in [
            ("1.toString", 2),
            ("1_0.x", 4),
            ("1.$", 2),
            ("1.name", 2),
            ("1._x", 2),
            ("1.\u{e4}", 2),
        ] {
            let (tokens, diags) = tokenize_all(source);
            assert_eq!(diags.len(), 1, "diags for {source:?}: {diags:?}");
            assert!(
                diags[0].message.contains("is a complete number"),
                "message for {source:?}: {diags:?}"
            );
            assert_eq!(diags[0].span, Span::new(F, 0, literal_end).unwrap());
            // The `.` is left as a member access, so the rest parses as written.
            assert_eq!(tokens[1].kind, TokenKind::Dot, "tokens for {source:?}");
        }
        let (_, diags) = tokenize_all("1.toString");
        assert_eq!(
            diags[0].help,
            vec!["wrap the number in parentheses, `(1).toString`, or write `1..toString`"]
        );
        let (_, diags) = tokenize_all("1.\u{e4}");
        assert_eq!(
            diags[0].help,
            vec!["wrap the number in parentheses, `(1).\u{e4}`, or write `1..\u{e4}`"]
        );
        // Repeating a literal that is itself an error would suggest broken code.
        let (_, diags) = tokenize_all("1_.x");
        assert_eq!(diags.len(), 2, "diags: {diags:?}");
        assert_eq!(
            diags[1].help,
            vec!["wrap the number in parentheses, or write a second `.`"]
        );
    }

    #[test]
    fn lex_legacy_octal_leaves_decimal_point() {
        let (tokens, diags) = tokenize_all("010.toString");
        assert_eq!(diags.len(), 1, "diags: {diags:?}");
        assert!(diags[0].message.contains("legacy octal literal `010`"));
        assert_eq!(tokens[1].kind, TokenKind::Dot);
    }

    #[test]
    fn lex_hex_trailing_junk_splits() {
        // `0xfg` lexes as `0xf` (15) then identifier `g`, mirroring `123abc`.
        let (tokens, diags) = tokenize_all("0xfg");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert!(matches!(tokens[0].kind, TokenKind::NumberLiteral(v) if v == 15.0));
        assert_eq!(tokens[0].span, Span::new(F, 0, 3).unwrap());
        assert_eq!(tokens[1].kind, TokenKind::Identifier);
        assert_eq!(tokens[1].span, Span::new(F, 3, 4).unwrap());
    }

    #[test]
    fn lex_bigint_then_dot_method() {
        let (tokens, diags) = tokenize_all("42n.toString()");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let kinds: Vec<_> = tokens.iter().map(|t| &t.kind).collect();
        assert!(matches!(kinds[0], TokenKind::BigIntLiteral(s) if s == "42"));
        assert_eq!(tokens[0].span, Span::new(F, 0, 3).unwrap());
        assert_eq!(tokens[1].kind, TokenKind::Dot);
    }

    #[test]
    fn string_double_quoted() {
        expect_string("\"hello\"", "hello");
    }

    #[test]
    fn string_single_quoted() {
        expect_string("'world'", "world");
    }

    #[test]
    fn string_escaped_double_quote() {
        expect_string("\"with \\\"escape\\\"\"", "with \"escape\"");
    }

    #[test]
    fn string_escaped_single_quote() {
        expect_string("'it\\'s'", "it's");
    }

    #[test]
    fn string_newline_and_tab_escapes() {
        expect_string("\"\\n\\t\"", "\n\t");
    }

    #[test]
    fn string_carriage_return_escape() {
        expect_string("\"a\\rb\"", "a\rb");
    }

    #[test]
    fn string_null_and_other_escapes() {
        expect_string("\"\\0\\b\\f\\v\"", "\0\u{08}\u{0C}\u{0B}");
    }

    #[test]
    fn string_backslash_escape() {
        expect_string("\"a\\\\b\"", "a\\b");
    }

    #[test]
    fn string_unicode_4hex_escape() {
        expect_string("\"\\u00e9\"", "é");
    }

    #[test]
    fn string_unicode_brace_escape() {
        expect_string("\"\\u{1F600}\"", "\u{1F600}");
    }

    #[test]
    fn string_surrogate_pair_joins() {
        // 😀 = U+1F600. Surrogate pair: D83D DE00.
        expect_string("\"\\uD83D\\uDE00\"", "\u{1F600}");
    }

    #[test]
    fn string_contains_multibyte_literal() {
        expect_string("\"café\"", "café");
    }

    #[test]
    fn string_unterminated_at_eof() {
        let (tok, _eof, diags) = tokenize_one("\"hello");
        assert!(matches!(tok.kind, TokenKind::StringLiteral(ref s) if s == "hello"));
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "unterminated string literal");
    }

    #[test]
    fn string_raw_newline_is_part_of_the_value() {
        let (tok, _eof, diags) = tokenize_one("\"a\nb\"");
        assert!(matches!(tok.kind, TokenKind::StringLiteral(ref s) if s == "a\nb"));
        assert!(diags.is_empty());
    }

    #[test]
    fn string_raw_crlf_normalised_to_lf() {
        let (tok, _eof, diags) = tokenize_one("\"a\r\nb\"");
        assert!(matches!(tok.kind, TokenKind::StringLiteral(ref s) if s == "a\nb"));
        assert!(diags.is_empty());
    }

    #[test]
    fn string_still_unterminated_when_newline_reaches_eof() {
        let (tok, _eof, diags) = tokenize_one("\"abc\n");
        assert!(matches!(tok.kind, TokenKind::StringLiteral(ref s) if s == "abc\n"));
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "unterminated string literal");
    }

    #[test]
    fn string_unknown_escape_diagnoses_but_keeps_character() {
        let (tok, _eof, diags) = tokenize_one("\"\\q\"");
        assert!(matches!(tok.kind, TokenKind::StringLiteral(ref s) if s == "q"));
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "unknown escape sequence `\\q`");
    }

    #[test]
    fn string_escaped_slash_is_a_slash() {
        let (tok, _eof, diags) = tokenize_one("\"a\\/b\"");
        assert!(matches!(tok.kind, TokenKind::StringLiteral(ref s) if s == "a/b"));
        assert!(diags.is_empty());
    }

    #[test]
    fn string_lone_high_surrogate_diagnosed() {
        let (_, _, diags) = tokenize_one("\"\\uD83D\"");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "lone high surrogate");
    }

    #[test]
    fn string_lone_low_surrogate_diagnosed() {
        let (_, _, diags) = tokenize_one("\"\\uDE00\"");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "lone low surrogate");
    }

    #[test]
    fn string_brace_escape_empty_diagnosed() {
        let (_, _, diags) = tokenize_one("\"\\u{}\"");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.starts_with("invalid unicode escape"));
    }

    #[test]
    fn string_brace_escape_too_big_diagnosed() {
        let (_, _, diags) = tokenize_one("\"\\u{110000}\"");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("exceeds U+10FFFF"));
    }

    #[test]
    fn string_brace_escape_unclosed_diagnosed() {
        let (_, _, diags) = tokenize_one("\"\\u{1F600\"");
        // At minimum the first diagnostic is about the unicode escape; unterminated string may follow.
        assert!(!diags.is_empty());
        assert!(diags[0].message.contains("invalid unicode escape"));
    }

    #[test]
    fn renders_unterminated_string_diagnostic() {
        let source = "let s = \"hello";
        let (_, diags) = tokenize_all(source);
        let unterminated = diags
            .iter()
            .find(|d| d.message == "unterminated string literal")
            .expect("expected an unterminated-string diagnostic");
        let rendered = diagnostics::render(unterminated, &sources(source));
        insta::assert_snapshot!(rendered);
    }

    fn expect_template(source: &str, expected: &[TokenKind]) {
        let (tokens, diags) = tokenize_all(source);
        assert!(
            diags.is_empty(),
            "unexpected diagnostics for {source:?}: {diags:?}"
        );
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind.clone()).collect();
        let mut want = expected.to_vec();
        want.push(TokenKind::Eof);
        assert_eq!(kinds, want, "token stream mismatch for {source:?}");
    }

    #[test]
    fn template_no_substitution_plain() {
        expect_template(
            "`hello`",
            &[TokenKind::TemplateNoSubstitution("hello".to_string())],
        );
    }

    #[test]
    fn template_no_substitution_empty() {
        expect_template("``", &[TokenKind::TemplateNoSubstitution(String::new())]);
    }

    #[test]
    fn template_no_substitution_with_dollar_not_followed_by_brace() {
        expect_template(
            "`a$b`",
            &[TokenKind::TemplateNoSubstitution("a$b".to_string())],
        );
    }

    #[test]
    fn template_single_interpolation() {
        expect_template(
            "`a${x}b`",
            &[
                TokenKind::TemplateHead("a".to_string()),
                TokenKind::Identifier,
                TokenKind::TemplateTail("b".to_string()),
            ],
        );
    }

    #[test]
    fn template_two_interpolations() {
        expect_template(
            "`a${x}b${y}c`",
            &[
                TokenKind::TemplateHead("a".to_string()),
                TokenKind::Identifier,
                TokenKind::TemplateMiddle("b".to_string()),
                TokenKind::Identifier,
                TokenKind::TemplateTail("c".to_string()),
            ],
        );
    }

    #[test]
    fn template_empty_head_and_tail() {
        expect_template(
            "`${x}`",
            &[
                TokenKind::TemplateHead(String::new()),
                TokenKind::Identifier,
                TokenKind::TemplateTail(String::new()),
            ],
        );
    }

    #[test]
    fn template_object_literal_inside_interpolation_does_not_close() {
        // `{` and `}` inside `${ }` adjust the frame counter, not the interpolation boundary.
        let (tokens, diags) = tokenize_all("`x=${ {a:1} }`");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind.clone()).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::TemplateHead("x=".to_string()),
                TokenKind::LeftBrace,
                TokenKind::Identifier,
                TokenKind::Colon,
                TokenKind::NumberLiteral(1.0),
                TokenKind::RightBrace,
                TokenKind::TemplateTail(String::new()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn template_nested_in_interpolation() {
        // Nested template: outer pushes frame 0, inner backtick pushes frame 1;
        // each `}` pops back to its level. Tests multi-frame correctness.
        expect_template(
            "`out${`in${x}`}end`",
            &[
                TokenKind::TemplateHead("out".to_string()),
                TokenKind::TemplateHead("in".to_string()),
                TokenKind::Identifier,
                TokenKind::TemplateTail(String::new()),
                TokenKind::TemplateTail("end".to_string()),
            ],
        );
    }

    #[test]
    fn template_escapes_inside_part() {
        // \` and \$ are template-only; others delegate to read_escape.
        expect_template(
            "`a\\nb\\`c\\${d}\\\\e\\u{1F600}`",
            &[TokenKind::TemplateNoSubstitution(
                "a\nb`c${d}\\e\u{1F600}".to_string(),
            )],
        );
    }

    #[test]
    fn template_multiline_lf_preserved() {
        expect_template(
            "`line1\nline2`",
            &[TokenKind::TemplateNoSubstitution(
                "line1\nline2".to_string(),
            )],
        );
    }

    #[test]
    fn template_multiline_crlf_normalised_to_lf() {
        expect_template(
            "`a\r\nb`",
            &[TokenKind::TemplateNoSubstitution("a\nb".to_string())],
        );
    }

    #[test]
    fn template_unterminated_no_substitution_diagnosed() {
        let (tokens, diags) = tokenize_all("`hello");
        assert_eq!(diags.len(), 1, "diags: {diags:?}");
        assert_eq!(diags[0].message, "unterminated template literal");
        assert!(matches!(
            &tokens[0].kind,
            TokenKind::TemplateNoSubstitution(s) if s == "hello"
        ));
    }

    #[test]
    fn template_unterminated_after_head_diagnosed() {
        let (_, diags) = tokenize_all("`a${x");
        // The interpolation is still open at EOF — lexer is not in template-scanning mode,
        // so no unterminated-template diagnostic; the parser catches the unclosed `${`.
        assert!(diags.is_empty(), "unexpected lexer diags: {diags:?}");
    }

    #[test]
    fn template_unterminated_after_interpolation_diagnosed() {
        let (_, diags) = tokenize_all("`a${x}b");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "unterminated template literal");
    }

    #[test]
    fn renders_unterminated_template_diagnostic() {
        let source = "let s = `hello";
        let (_, diags) = tokenize_all(source);
        let unterminated = diags
            .iter()
            .find(|d| d.message == "unterminated template literal")
            .expect("expected an unterminated-template diagnostic");
        let rendered = diagnostics::render(unterminated, &sources(source));
        insta::assert_snapshot!(rendered);
    }

    #[test]
    fn lex_true_false_null() {
        let (tokens, diags) = tokenize_all("true false null");
        assert!(diags.is_empty());
        assert_eq!(tokens[0].kind, TokenKind::BooleanLiteral(true));
        assert_eq!(tokens[1].kind, TokenKind::BooleanLiteral(false));
        assert_eq!(tokens[2].kind, TokenKind::NullLiteral);
        assert_eq!(tokens[3].kind, TokenKind::Eof);
    }

    #[test]
    fn literal_prefix_words_are_identifiers() {
        for src in ["trueValue", "falseness", "nullable"] {
            let (tok, _eof, diags) = tokenize_one(src);
            assert!(diags.is_empty(), "diagnostics for {src:?}");
            assert_eq!(
                tok.kind,
                TokenKind::Identifier,
                "expected Identifier for {src:?}"
            );
        }
    }

    #[test]
    fn lex_all_mvp_keywords() {
        let keywords = [
            ("let", TokenKind::Let),
            ("const", TokenKind::Const),
            ("function", TokenKind::Function),
            ("if", TokenKind::If),
            ("else", TokenKind::Else),
            ("while", TokenKind::While),
            ("return", TokenKind::Return),
            ("void", TokenKind::Void),
            ("interface", TokenKind::Interface),
            ("export", TokenKind::Export),
            ("typeof", TokenKind::Typeof),
            ("import", TokenKind::Import),
            ("new", TokenKind::New),
        ];
        for (src, expected) in keywords {
            let (tok, _eof, diags) = tokenize_one(src);
            assert!(diags.is_empty(), "diagnostics for {src:?}");
            assert_eq!(tok.kind, expected, "kind mismatch for {src:?}");
            assert_eq!(tok.span, Span::new(F, 0, src.len() as u32).unwrap());
        }
    }

    #[test]
    fn contextual_keywords_lex_as_identifiers() {
        for src in ["type", "is", "from", "as", "of"] {
            let (tok, _eof, diags) = tokenize_one(src);
            assert!(diags.is_empty(), "diagnostics for {src:?}");
            assert_eq!(tok.kind, TokenKind::Identifier, "kind mismatch for {src:?}");
        }
    }

    #[test]
    fn lex_ascii_identifiers() {
        for src in ["foo", "_bar", "$baz", "x1", "_123", "camelCase"] {
            let (tok, _eof, diags) = tokenize_one(src);
            assert!(diags.is_empty(), "diagnostics for {src:?}");
            assert_eq!(tok.kind, TokenKind::Identifier, "kind mismatch for {src:?}");
            assert_eq!(tok.span, Span::new(F, 0, src.len() as u32).unwrap());
        }
    }

    #[test]
    fn lex_unicode_identifier() {
        let (tok, _eof, diags) = tokenize_one("café");
        assert!(diags.is_empty());
        assert_eq!(tok.kind, TokenKind::Identifier);
        assert_eq!(tok.span, Span::new(F, 0, "café".len() as u32).unwrap());
    }

    #[test]
    fn keyword_prefix_is_identifier() {
        let (tok, _eof, diags) = tokenize_one("letx");
        assert!(diags.is_empty());
        assert_eq!(tok.kind, TokenKind::Identifier);
    }

    #[test]
    fn lex_arithmetic_operators() {
        expect_single_token("+", TokenKind::Plus, Span::new(F, 0, 1).unwrap());
        expect_single_token("-", TokenKind::Minus, Span::new(F, 0, 1).unwrap());
        expect_single_token("*", TokenKind::Star, Span::new(F, 0, 1).unwrap());
        // `/` after an identifier is division; standalone it would start a regex literal.
        let (toks, diags) = tokenize_all("a / b");
        assert!(diags.is_empty(), "diags: {diags:?}");
        assert_eq!(toks[1].kind, TokenKind::Slash);
        expect_single_token("%", TokenKind::Percent, Span::new(F, 0, 1).unwrap());
    }

    #[test]
    fn lex_compound_assignment_operators() {
        expect_single_token("+=", TokenKind::PlusEquals, Span::new(F, 0, 2).unwrap());
        expect_single_token("-=", TokenKind::MinusEquals, Span::new(F, 0, 2).unwrap());
        expect_single_token("*=", TokenKind::StarEquals, Span::new(F, 0, 2).unwrap());
        // `/=` after an identifier is compound-assign; standalone it would start a regex literal.
        let (toks, diags) = tokenize_all("a /= 2");
        assert!(diags.is_empty(), "diags: {diags:?}");
        assert_eq!(toks[1].kind, TokenKind::SlashEquals);
        expect_single_token("%=", TokenKind::PercentEquals, Span::new(F, 0, 2).unwrap());
        expect_single_token("**", TokenKind::StarStar, Span::new(F, 0, 2).unwrap());
        expect_single_token(
            "**=",
            TokenKind::StarStarEquals,
            Span::new(F, 0, 3).unwrap(),
        );
        // `**=` must beat `**` + `=` in greedy dispatch.
        let (toks, diags) = tokenize_all("a **= 2");
        assert!(diags.is_empty(), "diags: {diags:?}");
        assert_eq!(toks[1].kind, TokenKind::StarStarEquals);
        let (toks, diags) = tokenize_all("a ** b");
        assert!(diags.is_empty(), "diags: {diags:?}");
        assert_eq!(toks[1].kind, TokenKind::StarStar);
    }

    #[test]
    fn lex_regex_literal_basic() {
        // At start of input `last_significant_token` is `None`; `is_regex_context` returns true.
        expect_single_token(
            "/abc/",
            TokenKind::RegexLiteral {
                source: "abc".to_string(),
                flags: String::new(),
            },
            Span::new(F, 0, 5).unwrap(),
        );
    }

    #[test]
    fn lex_regex_literal_with_flags() {
        let (toks, diags) = tokenize_all("/foo/gi");
        assert!(diags.is_empty(), "diags: {diags:?}");
        assert_eq!(
            toks[0].kind,
            TokenKind::RegexLiteral {
                source: "foo".to_string(),
                flags: "gi".to_string(),
            }
        );
    }

    #[test]
    fn lex_regex_after_open_paren_is_literal() {
        let (toks, diags) = tokenize_all("f(/x/)");
        assert!(diags.is_empty(), "diags: {diags:?}");
        // [f, `(`, /x/, `)`, Eof]
        assert_eq!(toks[0].kind, TokenKind::Identifier);
        assert_eq!(toks[1].kind, TokenKind::LeftParen);
        assert!(matches!(toks[2].kind, TokenKind::RegexLiteral { .. }));
        assert_eq!(toks[3].kind, TokenKind::RightParen);
    }

    #[test]
    fn lex_regex_after_equals_is_literal() {
        let (toks, diags) = tokenize_all("let r = /a/g;");
        assert!(diags.is_empty(), "diags: {diags:?}");
        assert!(toks.iter().any(|t| matches!(
            &t.kind,
            TokenKind::RegexLiteral { source, flags } if source == "a" && flags == "g"
        )));
    }

    #[test]
    fn lex_regex_after_return_is_literal() {
        let (toks, diags) = tokenize_all("return /x/;");
        assert!(diags.is_empty(), "diags: {diags:?}");
        assert_eq!(toks[0].kind, TokenKind::Return);
        assert!(matches!(toks[1].kind, TokenKind::RegexLiteral { .. }));
    }

    #[test]
    fn lex_regex_after_typeof_is_literal() {
        let (toks, diags) = tokenize_all("typeof /x/");
        assert!(diags.is_empty(), "diags: {diags:?}");
        assert_eq!(toks[0].kind, TokenKind::Typeof);
        assert!(matches!(toks[1].kind, TokenKind::RegexLiteral { .. }));
    }

    #[test]
    fn lex_division_after_identifier_stays_division() {
        let (toks, diags) = tokenize_all("a / b");
        assert!(diags.is_empty(), "diags: {diags:?}");
        assert_eq!(toks[0].kind, TokenKind::Identifier);
        assert_eq!(toks[1].kind, TokenKind::Slash);
        assert_eq!(toks[2].kind, TokenKind::Identifier);
    }

    #[test]
    fn lex_division_after_close_paren_stays_division() {
        let (toks, diags) = tokenize_all("(x) / 2");
        assert!(diags.is_empty(), "diags: {diags:?}");
        assert!(toks.iter().any(|t| t.kind == TokenKind::Slash));
        assert!(
            !toks
                .iter()
                .any(|t| matches!(t.kind, TokenKind::RegexLiteral { .. }))
        );
    }

    #[test]
    fn lex_division_chain_a_div_b_div_c() {
        let (toks, diags) = tokenize_all("a / b / c");
        assert!(diags.is_empty(), "diags: {diags:?}");
        let kinds: Vec<&TokenKind> = toks.iter().map(|t| &t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                &TokenKind::Identifier,
                &TokenKind::Slash,
                &TokenKind::Identifier,
                &TokenKind::Slash,
                &TokenKind::Identifier,
                &TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn lex_regex_escaped_slash_does_not_close() {
        let (toks, diags) = tokenize_all("/a\\/b/");
        assert!(diags.is_empty(), "diags: {diags:?}");
        assert_eq!(
            toks[0].kind,
            TokenKind::RegexLiteral {
                source: "a\\/b".to_string(),
                flags: String::new(),
            }
        );
    }

    #[test]
    fn lex_regex_slash_inside_char_class_does_not_close() {
        let (toks, diags) = tokenize_all("/[/]/");
        assert!(diags.is_empty(), "diags: {diags:?}");
        assert_eq!(
            toks[0].kind,
            TokenKind::RegexLiteral {
                source: "[/]".to_string(),
                flags: String::new(),
            }
        );
    }

    #[test]
    fn lex_regex_unterminated_at_newline_is_diagnosed() {
        let (toks, diags) = tokenize_all("/abc\n");
        assert!(
            !diags.is_empty()
                && diags
                    .iter()
                    .any(|d| d.message.contains("unterminated regex literal")),
            "expected unterminated diagnostic, got {diags:?}"
        );
        assert!(matches!(toks[0].kind, TokenKind::RegexLiteral { .. }));
    }

    #[test]
    fn lex_regex_unterminated_at_eof_is_diagnosed() {
        let (_toks, diags) = tokenize_all("/abc");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("unterminated regex literal")),
            "expected unterminated diagnostic, got {diags:?}"
        );
    }

    #[test]
    fn lex_regex_after_regex_is_division() {
        // A regex literal is an operand; the `/` that follows is division.
        let (toks, diags) = tokenize_all("/x/ / 2");
        assert!(diags.is_empty(), "diags: {diags:?}");
        assert!(matches!(toks[0].kind, TokenKind::RegexLiteral { .. }));
        assert_eq!(toks[1].kind, TokenKind::Slash);
    }

    #[test]
    fn lex_block_comment_remains_unaffected() {
        let (toks, diags) = tokenize_all("/* a / b */ x");
        assert!(diags.is_empty(), "diags: {diags:?}");
        assert_eq!(toks[0].kind, TokenKind::Identifier);
    }

    #[test]
    fn lex_line_comment_remains_unaffected() {
        let (toks, diags) = tokenize_all("// /x/\n");
        assert!(diags.is_empty(), "diags: {diags:?}");
        // Line comment + newline are trivia; only Newline and Eof tokens remain.
        assert!(
            toks.iter()
                .all(|t| matches!(t.kind, TokenKind::Newline | TokenKind::Eof))
        );
        assert!(
            !toks
                .iter()
                .any(|t| matches!(t.kind, TokenKind::RegexLiteral { .. }))
        );
    }

    #[test]
    fn lex_postfix_increment_decrement() {
        // `++` must beat `+=` in greedy dispatch; `--` must beat `-=`.
        expect_single_token("++", TokenKind::PlusPlus, Span::new(F, 0, 2).unwrap());
        expect_single_token("--", TokenKind::MinusMinus, Span::new(F, 0, 2).unwrap());
        let (toks, diags) = tokenize_all("+=");
        assert!(diags.is_empty());
        assert_eq!(toks[0].kind, TokenKind::PlusEquals);
        let (toks, diags) = tokenize_all("-=");
        assert!(diags.is_empty());
        assert_eq!(toks[0].kind, TokenKind::MinusEquals);
        // Three pluses lex as `++ +` (greedy longest-match grabs the first two).
        let (toks, diags) = tokenize_all("+++");
        assert!(diags.is_empty());
        assert_eq!(toks[0].kind, TokenKind::PlusPlus);
        assert_eq!(toks[1].kind, TokenKind::Plus);
    }

    #[test]
    fn lex_equality_operators_longest_match() {
        expect_single_token("=", TokenKind::Equals, Span::new(F, 0, 1).unwrap());
        expect_single_token("==", TokenKind::EqEq, Span::new(F, 0, 2).unwrap());
        expect_single_token("===", TokenKind::EqEqEq, Span::new(F, 0, 3).unwrap());
        expect_single_token("!", TokenKind::Bang, Span::new(F, 0, 1).unwrap());
        expect_single_token("!=", TokenKind::BangEq, Span::new(F, 0, 2).unwrap());
        expect_single_token("!==", TokenKind::BangEqEq, Span::new(F, 0, 3).unwrap());
    }

    #[test]
    fn lex_arrow_token() {
        expect_single_token("=>", TokenKind::Arrow, Span::new(F, 0, 2).unwrap());
    }

    #[test]
    fn lex_comparison_operators() {
        expect_single_token("<", TokenKind::LessThan, Span::new(F, 0, 1).unwrap());
        expect_single_token(">", TokenKind::GreaterThan, Span::new(F, 0, 1).unwrap());
        expect_single_token("<=", TokenKind::LessEquals, Span::new(F, 0, 2).unwrap());
        expect_single_token(">=", TokenKind::GreaterEquals, Span::new(F, 0, 2).unwrap());
    }

    #[test]
    fn lex_logical_operators() {
        expect_single_token("&&", TokenKind::AmpAmp, Span::new(F, 0, 2).unwrap());
        expect_single_token("||", TokenKind::PipePipe, Span::new(F, 0, 2).unwrap());
    }

    #[test]
    fn lex_pipe_longest_match() {
        expect_single_token("|", TokenKind::Pipe, Span::new(F, 0, 1).unwrap());
        expect_single_token("||", TokenKind::PipePipe, Span::new(F, 0, 2).unwrap());
    }

    #[test]
    fn lex_dot_standalone_and_after_identifier() {
        expect_single_token(".", TokenKind::Dot, Span::new(F, 0, 1).unwrap());
        let (tokens, diags) = tokenize_all("foo.bar");
        assert!(diags.is_empty());
        assert_eq!(tokens[0].kind, TokenKind::Identifier);
        assert_eq!(tokens[1].kind, TokenKind::Dot);
        assert_eq!(tokens[2].kind, TokenKind::Identifier);
    }

    #[test]
    fn single_amp_is_unexpected() {
        let (_, _, diags) = tokenize_one("&");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("unexpected character"));
    }

    #[test]
    fn lex_all_delimiters() {
        expect_single_token("(", TokenKind::LeftParen, Span::new(F, 0, 1).unwrap());
        expect_single_token(")", TokenKind::RightParen, Span::new(F, 0, 1).unwrap());
        expect_single_token("{", TokenKind::LeftBrace, Span::new(F, 0, 1).unwrap());
        expect_single_token("}", TokenKind::RightBrace, Span::new(F, 0, 1).unwrap());
        expect_single_token("[", TokenKind::LeftBracket, Span::new(F, 0, 1).unwrap());
        expect_single_token("]", TokenKind::RightBracket, Span::new(F, 0, 1).unwrap());
        expect_single_token(",", TokenKind::Comma, Span::new(F, 0, 1).unwrap());
        expect_single_token(":", TokenKind::Colon, Span::new(F, 0, 1).unwrap());
        expect_single_token(";", TokenKind::Semicolon, Span::new(F, 0, 1).unwrap());
        expect_single_token("?", TokenKind::Question, Span::new(F, 0, 1).unwrap());
    }

    #[test]
    fn lex_brackets_combined() {
        let (tokens, diags) = tokenize_all("({[]})");
        assert!(diags.is_empty());
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind.clone()).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::LeftParen,
                TokenKind::LeftBrace,
                TokenKind::LeftBracket,
                TokenKind::RightBracket,
                TokenKind::RightBrace,
                TokenKind::RightParen,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn lex_newline_lf() {
        expect_single_token("\n", TokenKind::Newline, Span::new(F, 0, 1).unwrap());
    }

    #[test]
    fn lex_newline_crlf_is_one_token() {
        expect_single_token("\r\n", TokenKind::Newline, Span::new(F, 0, 2).unwrap());
    }

    #[test]
    fn lex_newline_cr() {
        expect_single_token("\r", TokenKind::Newline, Span::new(F, 0, 1).unwrap());
    }

    #[test]
    fn lex_mixed_newlines() {
        let (tokens, diags) = tokenize_all("a\nb\r\nc\rd");
        assert!(diags.is_empty());
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind.clone()).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Identifier,
                TokenKind::Newline,
                TokenKind::Identifier,
                TokenKind::Newline,
                TokenKind::Identifier,
                TokenKind::Newline,
                TokenKind::Identifier,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn line_comment_skipped_but_newline_preserved() {
        let (tokens, diags) = tokenize_all("a // comment\nb");
        assert!(diags.is_empty());
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind.clone()).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Identifier,
                TokenKind::Newline,
                TokenKind::Identifier,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn block_comment_skipped() {
        let (tokens, diags) = tokenize_all("a /* block */ b");
        assert!(diags.is_empty());
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind.clone()).collect();
        assert_eq!(
            kinds,
            vec![TokenKind::Identifier, TokenKind::Identifier, TokenKind::Eof]
        );
    }

    #[test]
    fn empty_block_comment_is_fine() {
        let (tokens, diags) = tokenize_all("/**/");
        assert!(diags.is_empty());
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::Eof);
    }

    #[test]
    fn unterminated_block_comment_diagnosed() {
        let (_, diags) = tokenize_all("/* unterminated");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "unterminated block comment");
        assert_eq!(diags[0].span, Span::new(F, 0, 2).unwrap());
    }

    #[test]
    fn single_slash_star_not_block_comment_start() {
        // `/*/` is parsed as the start of a block comment with no terminator.
        let (_, diags) = tokenize_all("/*/");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "unterminated block comment");
    }

    #[test]
    fn doc_comment_attaches_to_next_token() {
        let (tok, eof, diags) = tokenize_one("/** Summary. */ foo");
        let doc = tok.leading_doc.as_ref().expect("doc attached");
        assert_eq!(doc.text, "/** Summary. */");
        assert_eq!(doc.span, Span::new(F, 0, 15).unwrap());
        assert_eq!(tok.kind, TokenKind::Identifier);
        assert!(eof.leading_doc.is_none());
        assert!(diags.is_empty());
    }

    #[test]
    fn doc_comment_survives_intervening_newlines() {
        let (tokens, diags) = tokenize_all("/** doc */\n\nfoo");
        assert!(diags.is_empty());
        let ident = tokens
            .iter()
            .find(|t| t.kind == TokenKind::Identifier)
            .expect("identifier present");
        assert!(ident.leading_doc.is_some());
        for nl in tokens.iter().filter(|t| t.kind == TokenKind::Newline) {
            assert!(nl.leading_doc.is_none(), "newline shouldn't claim the doc");
        }
    }

    #[test]
    fn empty_doc_comment_captured() {
        let (tok, _eof, _diags) = tokenize_one("/** */ x");
        let doc = tok.leading_doc.as_ref().expect("doc attached");
        assert_eq!(doc.text, "/** */");
    }

    #[test]
    fn regular_block_comment_not_captured() {
        let (tok, _eof, _diags) = tokenize_one("/* not a doc */ foo");
        assert!(tok.leading_doc.is_none());
    }

    #[test]
    fn empty_block_comment_not_a_doc() {
        let (tok, _eof, _diags) = tokenize_one("/**/ foo");
        assert!(tok.leading_doc.is_none());
    }

    #[test]
    fn line_comment_not_captured() {
        let (tok, _eof, _diags) = tokenize_one("// line\nfoo");
        assert!(tok.leading_doc.is_none());
    }

    #[test]
    fn last_doc_wins_when_multiple_in_a_row() {
        let (tok, _eof, _diags) = tokenize_one("/** first */ /** second */ foo");
        let doc = tok.leading_doc.as_ref().expect("doc attached");
        assert_eq!(doc.text, "/** second */");
    }

    #[test]
    fn unterminated_doc_emits_diagnostic() {
        let (tok, _eof, diags) = tokenize_one("/** unterminated");
        assert_eq!(tok.kind, TokenKind::Eof);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "unterminated block comment");
    }

    /// Editors commonly prepend U+FEFF; TypeScript ignores it, and so do we.
    #[test]
    fn leading_bom_is_skipped() {
        let (tokens, diags) = tokenize_all("\u{feff}const x = 1;");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(tokens[0].kind, TokenKind::Const);
    }

    /// The BOM is stepped over rather than stripped, so spans stay byte offsets into
    /// the file on disk — otherwise every caret after it would be three bytes off.
    #[test]
    fn leading_bom_keeps_spans_aligned_to_the_file() {
        let source = "\u{feff}const x = 1;";
        let (tokens, _diags) = tokenize_all(source);
        let start = tokens[0].span.start as usize;
        assert_eq!(&source[start..start + 5], "const");
    }

    /// Only offset 0 is an encoding marker. Elsewhere it is an ordinary invalid character.
    #[test]
    fn bom_after_the_first_byte_is_still_an_error() {
        let (_tokens, diags) = tokenize_all("const \u{feff}x = 1;");
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0].message.contains("unexpected character"),
            "got: {}",
            diags[0].message
        );
    }

    #[test]
    fn tokenize_mvp_fixture() {
        let source = "function greet(name: string): string {\n  return \"Hello, \" + name;\n}\n\nfunction main(): string {\n  const msg = greet(\"world\");\n  console.log(msg);\n  assert(msg === \"Hello, world\", \"greeting should match\");\n  return msg;\n}\n";
        let (tokens, diags) = tokenize_all(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let kinds: Vec<String> = tokens.iter().map(|t| format!("{:?}", t.kind)).collect();
        insta::assert_debug_snapshot!(kinds);
    }
}
