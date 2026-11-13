//! The syntax vocabulary a typed tree shares with the parser: identifiers,
//! operators, binding kinds, visibility and the arena ids of the parser tree.

use crate::Span;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExprId(pub u32);

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StmtId(pub u32);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

/// Postfix only — prefix `++x`/`--x` is not supported.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum PostfixOp {
    Inc,
    Dec,
    NonNullAssert,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    /// `a ** b` — right-associative exponentiation.
    Pow,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    UnsignedShr,
    Eq,
    NotEq,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
    In,
    /// `a ?? b` — nullish coalescing. Parser rejects mixing with `||`/`&&` without parentheses.
    NullishCoalesce,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum UnOp {
    Not,
    BitNot,
    Neg,
    Pos,
}

impl BinOp {
    pub fn bitwise_name(self) -> Option<&'static str> {
        match self {
            Self::BitAnd => Some("bitand"),
            Self::BitOr => Some("bitor"),
            Self::BitXor => Some("bitxor"),
            Self::Shl => Some("shl"),
            Self::Shr => Some("shr"),
            Self::UnsignedShr => Some("ushr"),
            _ => None,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum BindingKind {
    Let,
    Const,
}

/// Member visibility. No `protected` — it is rejected at parse time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Visibility {
    #[default]
    Public,
    Private,
}

impl Visibility {
    pub fn is_public(&self) -> bool {
        *self == Visibility::Public
    }
}
