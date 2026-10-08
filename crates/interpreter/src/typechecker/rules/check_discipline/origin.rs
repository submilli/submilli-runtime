//! What a caller-supplied value is, and which reads of one collide.

use std::fmt::Write;

use crate::typechecker::infer::narrowing::{KeyKind, LiteralValue, PathElem};
use crate::{ExprId, MangledName, Span};

/// A caller-supplied parameter: the one at `position` of the body itself, or
/// of the nested function `function` that calls `check()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct ParameterId {
    pub(super) function: Option<ExprId>,
    pub(super) position: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ValueRoot {
    Parameter(ParameterId),
    This,
    Global(MangledName),
    Unresolved(String),
}

/// The path a value was reached by. Aliases of one value share it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Origin {
    pub(super) root: ValueRoot,
    pub(super) steps: Vec<ReadKey>,
}

impl Origin {
    pub(super) fn of(root: ValueRoot) -> Self {
        Self {
            root,
            steps: Vec::new(),
        }
    }

    pub(super) fn is_bare_this(&self) -> bool {
        self.root == ValueRoot::This && self.steps.is_empty()
    }

    /// Whether one of the two may hold, or count the elements of, the other:
    /// along the shorter path, each step of one can read the same part as
    /// the step of the other, as `tags[0]` and an iteration of `tags` can,
    /// or `tags.length` and either of them.
    pub(super) fn is_related(&self, other: &Origin) -> bool {
        self.root == other.root
            && self
                .steps
                .iter()
                .zip(&other.steps)
                .all(|(step, other)| step.conflicts_with(other))
    }

    pub(super) fn after(&self, steps: &[ReadKey]) -> Self {
        let mut after = self.clone();
        after.steps.extend_from_slice(steps);
        after
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct CallerValue {
    pub(super) origin: Origin,
    /// The value as the source names it at this use.
    pub(super) shown: String,
    /// The depth of the loop the value came to exist in.
    pub(super) loop_depth: u32,
    /// Where the binding the value is used through is declared.
    pub(super) bound: Span,
}

impl CallerValue {
    /// The value a read of `key` yields.
    pub(super) fn after(&self, key: &ReadKey, loop_depth: u32) -> CallerValue {
        CallerValue {
            origin: self.origin.after(std::slice::from_ref(key)),
            shown: key.shown_on(&self.shown),
            loop_depth,
            bound: self.bound,
        }
    }
}

/// One read of a value, and one step of the path an [`Origin`] records.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ReadKey {
    Property(String),
    AnyProperty,
    Element(LiteralValue),
    AnyElement,
    Iteration,
}

#[derive(PartialEq, Eq)]
enum Family {
    Properties,
    Elements,
}

impl ReadKey {
    /// The read one element of a narrowed reference's path makes.
    pub(super) fn of_path(element: &PathElem) -> Self {
        match element {
            PathElem::Field(name) => ReadKey::Property(name.clone()),
            PathElem::Index(LiteralValue::String(name)) => ReadKey::Property(name.clone()),
            PathElem::Index(index) => ReadKey::Element(index.clone()),
            PathElem::Key(_, KeyKind::Property) => ReadKey::AnyProperty,
            PathElem::Key(_, KeyKind::Element) => ReadKey::AnyElement,
        }
    }

    /// Whether this read and `earlier`, of one value, can observe the same
    /// part of it. An array's `length` and a `Map`'s or `Set`'s `size` count
    /// the elements an iteration or an index reads, so a getter behind
    /// either can change the other.
    pub(super) fn conflicts_with(&self, earlier: &ReadKey) -> bool {
        if self.family() != earlier.family() {
            return self.is_count() && earlier.reads_elements()
                || earlier.is_count() && self.reads_elements();
        }
        match (self.named_part(), earlier.named_part()) {
            (Some(part), Some(earlier_part)) => part == earlier_part,
            (None, _) | (_, None) => true,
        }
    }

    pub(super) fn reads_elements(&self) -> bool {
        self.family() == Family::Elements
    }

    fn is_count(&self) -> bool {
        matches!(self, ReadKey::Property(name) if name == "length" || name == "size")
    }

    /// How the source names what the read yields, on a value it names `shown`.
    pub(super) fn shown_on(&self, shown: &str) -> String {
        let mut out = shown.to_string();
        // Writing to a `String` cannot fail.
        let _ = match self {
            ReadKey::Property(name) => write!(out, ".{name}"),
            ReadKey::Element(LiteralValue::Number(index)) => write!(out, "[{}]", index.0),
            ReadKey::Element(LiteralValue::String(key)) => write!(out, "[{key:?}]"),
            ReadKey::Element(LiteralValue::Boolean(key)) => write!(out, "[{key}]"),
            ReadKey::Element(LiteralValue::BigInt(digits)) => write!(out, "[{digits}n]"),
            ReadKey::AnyProperty | ReadKey::AnyElement => write!(out, "[...]"),
            ReadKey::Iteration => Ok(()),
        };
        out
    }

    fn family(&self) -> Family {
        match self {
            ReadKey::Property(_) | ReadKey::AnyProperty => Family::Properties,
            ReadKey::Element(_) | ReadKey::AnyElement | ReadKey::Iteration => Family::Elements,
        }
    }

    /// The one part a read names, or `None` when it may be any part.
    fn named_part(&self) -> Option<NamedPart<'_>> {
        match self {
            ReadKey::Property(name) => Some(NamedPart::Property(name)),
            ReadKey::Element(index) => Some(NamedPart::Element(index)),
            ReadKey::AnyProperty | ReadKey::AnyElement | ReadKey::Iteration => None,
        }
    }
}

#[derive(PartialEq)]
enum NamedPart<'a> {
    Property(&'a str),
    Element(&'a LiteralValue),
}
