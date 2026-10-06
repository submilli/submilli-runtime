use crate::{RawDoc, Span};

#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    NumberLiteral(f64),
    /// No `n` suffix, no leading sign (unary `-` is at the AST level).
    /// String because the value may exceed any fixed-width integer.
    BigIntLiteral(String),
    StringLiteral(String),
    BooleanLiteral(bool),
    NullLiteral,
    /// No `${…}` interpolations; parser collapses to a plain string literal.
    TemplateNoSubstitution(String),
    /// Cooked text up to the first `${`. Lexer pushes a frame onto
    /// `template_frames`; the next depth-0 `}` re-enters template scanning.
    TemplateHead(String),
    TemplateMiddle(String),
    /// Cooked text from the last `}` to the closing backtick.
    /// Pops the frame pushed by [`TemplateHead`](Self::TemplateHead).
    TemplateTail(String),
    /// `source` preserves escape sequences verbatim (the regex engine
    /// interprets them, not the lexer). Context disambiguation
    /// (literal vs. division) via `is_regex_context`.
    RegexLiteral {
        source: String,
        flags: String,
    },

    // Identifier (span-only; text via &source[span.start..span.end])
    Identifier,

    Let,
    Const,
    Function,
    If,
    Else,
    While,
    Do,
    For,
    Break,
    Continue,
    Return,
    Switch,
    Case,
    Default,
    /// Phase-1 forgiveness: accepted as a leading modifier on top-level
    /// declarations and ignored (there is no export consumer yet).
    Export,
    Void,
    Interface,
    Enum,
    /// Also drives the `"field" in x` narrowing predicate.
    In,
    Typeof,
    Import,
    Instanceof,
    New,
    Try,
    Catch,
    Finally,
    Throw,
    Class,
    Extends,
    Implements,
    Super,
    This,

    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Equals,
    PlusEquals,
    MinusEquals,
    StarEquals,
    SlashEquals,
    /// Lowered by the compound-assignment desugar to `x = x % y`.
    PercentEquals,
    /// Right-associative, higher precedence than `*`/`/`/`%`.
    /// Lex order: `**=` matched before `**` before `*=`.
    StarStar,
    StarStarEquals,
    /// Postfix-only (no prefix `++x`). Lex order: `++` matched before `+=`.
    PlusPlus,
    MinusMinus,
    EqEqEq,
    EqEq,
    BangEqEq,
    BangEq,
    LessThan,
    GreaterThan,
    LessEquals,
    GreaterEquals,
    Bang,
    AmpAmp,
    Amp,
    AmpEquals,
    PipeEquals,
    Caret,
    CaretEquals,
    Tilde,
    Pipe,
    PipePipe,
    Dot,
    /// Parser accepts only inside destructuring patterns (`{ … }` / `[ … ]`).
    DotDotDot,
    Arrow,
    Question,
    /// Single token so the postfix-chain parser dispatches on one peek.
    QuestionDot,
    QuestionQuestion,

    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,
    Comma,
    Colon,
    Semicolon,

    Newline,
    Eof,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    /// `/** ... */` doc comment captured as out-of-band trivia;
    /// consumed by the parser at declaration tokens.
    pub leading_doc: Option<RawDoc>,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Self {
            kind,
            span,
            leading_doc: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Token, TokenKind};
    use crate::Span;

    #[test]
    fn construct_literal_keyword_operator() {
        let num = Token::new(
            TokenKind::NumberLiteral(42.5),
            Span::new(crate::FileId(0), 0, 4).unwrap(),
        );
        assert_eq!(num.kind, TokenKind::NumberLiteral(42.5));
        assert_eq!(num.span, Span::new(crate::FileId(0), 0, 4).unwrap());
        assert!(num.leading_doc.is_none());

        let kw = Token::new(TokenKind::Let, Span::new(crate::FileId(0), 5, 8).unwrap());
        assert_eq!(kw.kind, TokenKind::Let);

        let op = Token::new(
            TokenKind::EqEqEq,
            Span::new(crate::FileId(0), 10, 13).unwrap(),
        );
        assert_eq!(op.kind, TokenKind::EqEqEq);
    }

    #[test]
    fn clone_and_equality() {
        let t = Token::new(
            TokenKind::StringLiteral("hello".to_string()),
            Span::new(crate::FileId(0), 0, 7).unwrap(),
        );
        assert_eq!(t.clone(), t);
    }
}
