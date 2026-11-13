//! What narrowing records in a typed tree: the paths it narrows and the
//! casts codegen emits for them.

use crate::{MangledName, Type, types::LiteralF64};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReferencePath {
    pub root: BindingId,
    pub chain: Vec<PathElem>,
}

impl ReferencePath {
    /// A local itself, with no field or element step. Only such a path can't
    /// change behind a guard's back, through an alias or in a call.
    pub fn is_bare_local(&self) -> bool {
        self.chain.is_empty() && matches!(self.root, BindingId::Local { .. })
    }

    pub fn root(root: BindingId) -> Self {
        Self {
            root,
            chain: Vec::new(),
        }
    }

    /// Whether a write to `self` may change what `other` reads. A key step
    /// of `other` (`o[key]`) may name any field or element, so any written
    /// step matches it. A written key matches only itself, as in
    /// TypeScript: `values[index] = 4` leaves `values[0]` narrowed.
    pub fn is_prefix_of(&self, other: &ReferencePath) -> bool {
        self.root == other.root
            && self.chain.len() <= other.chain.len()
            && self
                .chain
                .iter()
                .zip(&other.chain)
                .all(|(written, read)| read.is_written_by(written))
    }

    pub fn render(&self) -> String {
        let mut out = self.root.render();
        for elem in &self.chain {
            match elem {
                PathElem::Field(name) => {
                    out.push('.');
                    out.push_str(name);
                }
                PathElem::Key(binding, _) => {
                    out.push('[');
                    out.push_str(&binding.render());
                    out.push(']');
                }
                PathElem::Index(lit) => {
                    use std::fmt::Write;
                    match lit {
                        LiteralValue::Number(n) => {
                            let _ = write!(out, "[{}]", n.0);
                        }
                        LiteralValue::String(s) => {
                            let _ = write!(out, "[{s:?}]");
                        }
                        LiteralValue::Boolean(b) => {
                            let _ = write!(out, "[{b}]");
                        }
                        LiteralValue::BigInt(digits) => {
                            let _ = write!(out, "[{digits}n]");
                        }
                        LiteralValue::Null => {
                            let _ = write!(out, "[null]");
                        }
                        LiteralValue::Undefined => {
                            let _ = write!(out, "[undefined]");
                        }
                    }
                }
            }
        }
        out
    }
}

/// `decl_scope` on `Local` is non-optional for correctness: name-only keying
/// lets inner scopes incorrectly inherit outer narrowings.
///
/// `This` needs no payload: narrow scopes are per function body and a closure
/// boundary resets them, so within one body `this` names exactly one receiver.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BindingId {
    Local { name: String, decl_scope: ScopeId },
    Global(MangledName),
    This,
}

impl BindingId {
    /// The name the source reads the binding by.
    pub fn render(&self) -> String {
        match self {
            BindingId::Local { name, .. } => name.clone(),
            BindingId::Global(mangled) => mangled
                .as_str()
                .rsplit('#')
                .next()
                .unwrap_or(mangled.as_str())
                .to_string(),
            BindingId::This => "this".to_string(),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScopeId(pub u32);

/// `Index` is restricted to constant literal indices so paths stay hash-comparable;
/// `Key` indexes by a binding that holds one value for its whole life (a
/// `const`, or a parameter or `let` never assigned), as TypeScript narrows
/// `obj[key]`. Other index expressions are not narrowable.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PathElem {
    Field(String),
    Index(LiteralValue),
    Key(BindingId, KeyKind),
}

impl PathElem {
    fn is_written_by(&self, written: &PathElem) -> bool {
        matches!(self, PathElem::Key(..)) || self == written
    }
}

/// What a [`PathElem::Key`] reads: a property, by a string key, or an element.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeyKind {
    Property,
    Element,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LiteralValue {
    Number(LiteralF64),
    String(String),
    Boolean(bool),
    /// A bigint's canonical decimal digits, as [`Type::BigIntLiteral`] holds them.
    BigInt(String),
    /// Discriminant values only: a member may name its variant with `null` or
    /// `undefined`, and an optional discriminant is `undefined` when absent.
    Null,
    Undefined,
}

impl LiteralValue {
    /// The value an enum member literal type `E.A` holds.
    pub fn of_enum_member(ty: &Type) -> Option<Self> {
        Some(match ty.enum_member_value()? {
            crate::types::EnumValue::Number(value) => LiteralValue::Number(value),
            crate::types::EnumValue::String(value) => LiteralValue::String(value),
        })
    }
}

/// The cast is **unchecked**: codegen trusts the typechecker's narrowing decision. If the
/// predicate didn't actually prove what the narrowing claims, the Wasm-level cast traps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CastInfo {
    pub from_ty: Type,
    pub to_ty: Type,
    pub cast_kind: CastKind,
}

/// Dispatch hint for `emit_narrowing_cast`. Variants carry **no Wasm-level
/// indices** — those live in codegen's own registries.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CastKind {
    /// `T | null` → `T`. Codegen emits `ref.as_non_null`.
    NonNull,
    /// Boxed primitive → unboxed primitive value. Codegen emits an
    /// (optional) `ref.as_non_null` (when `from_ty` is nullable), a
    /// `ref.cast` to the wrapper subtype (`$BoxedNumber`/`$BoxedBoolean`),
    /// and `struct.get $value`.
    Unbox,
    /// Ref-typed value → more-specific ref subtype. Codegen emits `ref.cast`
    /// to the registered subtype when `to_ty`'s Wasm repr is tighter than
    /// `from_ty`'s; emits no instruction when reps match.
    RefSubtype,
}

/// Keep in sync with `emit_narrowing_cast` in `codegen/function_emitter/cast.rs`.
pub fn cast_info_for(from_ty: Type, narrowed_ty: Type) -> CastInfo {
    let cast_kind = match &narrowed_ty {
        Type::Number | Type::NumberLiteral(_) | Type::Boolean | Type::BooleanLiteral(_) => {
            CastKind::Unbox
        }
        Type::Object { .. } | Type::InterfaceRef { .. } | Type::ClassRef { .. } => {
            CastKind::RefSubtype
        }
        // unknown → ref-typed concrete shape uses the same downcast.
        Type::String
        | Type::StringLiteral(_)
        | Type::TemplateLiteral { .. }
        | Type::Array(_)
        | Type::Tuple(_)
        | Type::Uint8Array
            if matches!(from_ty.peel(), Type::Unknown) =>
        {
            CastKind::RefSubtype
        }
        _ => CastKind::NonNull,
    };
    CastInfo {
        from_ty,
        to_ty: narrowed_ty,
        cast_kind,
    }
}
