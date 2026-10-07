//! Narrowing engine — pure data structures.

use std::collections::{BTreeMap, BTreeSet};

use crate::{ExprId, Ident, MangledName, Type, types::LiteralF64};

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
}

/// Bitmask over narrowing predicates, modeled on TypeScript's `TypeFacts`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TypeFacts(pub u16);

impl TypeFacts {
    pub const EMPTY: Self = Self(0);
    pub const TRUTHY: Self = Self(1 << 0);
    pub const FALSY: Self = Self(1 << 1);
    pub const EQ_NULL: Self = Self(1 << 2);
    pub const NE_NULL: Self = Self(1 << 3);
    pub const IS_NUMBER: Self = Self(1 << 4);
    pub const IS_STRING: Self = Self(1 << 5);
    pub const IS_BOOLEAN: Self = Self(1 << 6);
    pub const IS_OBJECT: Self = Self(1 << 7);
    pub const IS_FUNCTION: Self = Self(1 << 8);
    pub const IS_ARRAY: Self = Self(1 << 9);

    pub fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl std::ops::BitOr for TypeFacts {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for TypeFacts {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl std::ops::BitAnd for TypeFacts {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

impl std::ops::Not for TypeFacts {
    type Output = Self;
    fn not(self) -> Self {
        const ALL: u16 = (1 << 10) - 1;
        Self((!self.0) & ALL)
    }
}

/// Pushed on loop/switch entry, popped on exit; collects break and continue narrow-state snapshots.
#[derive(Debug)]
pub struct PendingJoinFrame {
    pub kind: PendingJoinKind,
    /// `narrow_scopes.len()` at the moment the construct was
    /// entered, *before* the construct's body frame is pushed.
    /// Snapshots taken at `break`/`continue` time use this as the
    /// inclusive base — every frame at index >= `narrow_depth`
    /// was pushed inside the construct and is folded into the
    /// snapshot's narrow env (innermost wins on lookup).
    pub narrow_depth: usize,
    pub breaks: Vec<(NarrowEnv, BTreeSet<ReferencePath>)>,
    pub continues: Vec<(NarrowEnv, BTreeSet<ReferencePath>)>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PendingJoinKind {
    /// continue targets the innermost frame of this kind.
    Loop,
    /// Transparent to `continue` — search past this frame when finding the continue target.
    Switch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NarrowedView {
    pub narrowed_ty: Type,
    pub facts: TypeFacts,
    /// Literal values excluded by negative predicates (e.g., `s.kind !== "circle"`
    /// adds `"circle"` here).
    pub excluded_literals: BTreeSet<LiteralValue>,
    /// Shadow Wasm local binding name (`#narrow_<N>`) codegen uses for in-region
    /// references to this narrowed path.
    pub binding: Ident,
    /// Un-narrowed source expression codegen evaluates at `NarrowRegion` entry.
    /// Minted fresh — not shared with other AST positions (would double-trigger
    /// walker visits).
    pub source: ExprId,
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

/// One field-path narrowing that survived the `if`-join and needs a
/// fresh shadow Wasm local materialized at the join site.
#[derive(Clone, Debug)]
pub struct PendingPostIfMaterialization {
    pub path: ReferencePath,
    pub source: ExprId,
    pub binding: Ident,
    pub cast_info: CastInfo,
    pub span: crate::Span,
}

/// Diagnostic-only record of why a narrowing was invalidated. Surfaced as a
/// `help:` line so the LLM doesn't have to guess why a narrowing went away.
#[derive(Clone, Debug)]
pub enum InvalidationReason {
    /// A proven predicate had no source that codegen could rebuild in its region.
    ShapeUnrebuildable { narrowed_ty: Type },
    /// A write reached the path (or a prefix of it) and falsified the guard:
    /// a field or index write, a loop back edge, or a merge of an inner block's
    /// writes. Narrowings on the path and any extension of it are dropped.
    Write { span: crate::Span },
    /// An identifier was reassigned. Distinguished from [`Self::Write`] only so
    /// the diagnostic can name the statement the reader is looking at.
    Reassignment { span: crate::Span },
    /// Path root is in captured_mutators; the engine refused to install a NarrowedView.
    /// closure_span: one offending closure declaration.
    CapturedMutator { closure_span: Option<crate::Span> },
}

impl InvalidationReason {
    pub fn invalidates(&self) -> bool {
        !matches!(self, Self::ShapeUnrebuildable { .. })
    }

    pub fn span(&self) -> Option<crate::Span> {
        match self {
            Self::ShapeUnrebuildable { .. } => None,
            Self::Write { span } => Some(*span),
            Self::Reassignment { span } => Some(*span),
            Self::CapturedMutator { closure_span } => *closure_span,
        }
    }
}

/// A union's discriminant: the property every member types with a distinct
/// literal, and the map from that literal to the member it identifies.
pub type Discriminant = (String, BTreeMap<LiteralValue, VariantIdx>);

/// Variant index in a `Type::Union`'s canonical member list.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VariantIdx(pub u32);

/// Result of analyzing a `Type::Union` for a discriminant property.
///
/// `Some((key_property, constituent_map))` when there exists a property that:
///
/// - appears on every member of the union,
/// - has a unit (literal) type on every member,
/// - uniquely identifies each member.
///
/// `None` otherwise.
///
/// The key is returned as a bare `String` (not `Ident`) — the discriminant
/// property name has no meaningful source span at the union level; callers
/// compare it against `PathElem::Field(String)` from a `ReferencePath` anyway.
pub fn union_discriminant(members: &[Type]) -> Option<Discriminant> {
    // Inline object shapes only. A nominal member (interface, class) needs the
    // type registry to expand, which callers that have an `Inferer` supply
    // through [`discriminant_from_shapes`].
    //
    // Peel aliases so `type Shape = Circle | Rectangle` finds its discriminant.
    let shapes: Vec<BTreeMap<String, crate::types::ObjectField>> = members
        .iter()
        .map(|m| match m.peel() {
            Type::Object { fields, .. } => Some(fields.clone()),
            _ => None,
        })
        .collect::<Option<_>>()?;
    discriminant_from_shapes(&shapes)
}

/// [`union_discriminant`] over already-expanded member shapes — one map per
/// union member, in member order.
pub fn discriminant_from_shapes(
    shapes: &[BTreeMap<String, crate::types::ObjectField>],
) -> Option<Discriminant> {
    if shapes.len() < 2 {
        return None;
    }
    'next_key: for key in shapes[0].keys() {
        let mut map: BTreeMap<LiteralValue, VariantIdx> = BTreeMap::new();
        for (idx, shape) in shapes.iter().enumerate() {
            let Some(field) = shape.get(key) else {
                continue 'next_key;
            };
            // optional fields can't serve as discriminants —
            // an optional `kind?: "circle"` could be absent at
            // runtime, so the literal-equality predicate doesn't
            // uniquely identify the variant.
            if field.optional {
                continue 'next_key;
            }
            let Some(lit) = unit_literal_value(&field.ty) else {
                continue 'next_key;
            };
            if map.insert(lit, VariantIdx(idx as u32)).is_some() {
                continue 'next_key;
            }
        }
        return Some((key.clone(), map));
    }
    None
}

/// Tuple-union analog of [`union_discriminant`]: find a position whose element
/// type is a literal and pairwise distinct across variants.
/// Positions are tried only up to the smallest variant's arity.
pub fn tuple_union_discriminant(
    members: &[Type],
) -> Option<(usize, BTreeMap<LiteralValue, VariantIdx>)> {
    if members.len() < 2 {
        return None;
    }
    let tuples: Vec<&Vec<Type>> = members
        .iter()
        .map(|m| match m.peel() {
            Type::Tuple(elements) => Some(elements),
            _ => None,
        })
        .collect::<Option<_>>()?;
    let min_arity = tuples.iter().map(|t| t.len()).min().unwrap_or(0);
    'next_position: for position in 0..min_arity {
        let mut map: BTreeMap<LiteralValue, VariantIdx> = BTreeMap::new();
        for (idx, tuple) in tuples.iter().enumerate() {
            let Some(lit) = unit_literal_value(&tuple[position]) else {
                continue 'next_position;
            };
            if map.insert(lit, VariantIdx(idx as u32)).is_some() {
                continue 'next_position;
            }
        }
        return Some((position, map));
    }
    None
}

/// The literal type a compared literal value has.
pub fn literal_type(literal: &LiteralValue) -> Type {
    match literal {
        LiteralValue::String(s) => Type::StringLiteral(s.clone()),
        LiteralValue::Number(n) => Type::NumberLiteral(*n),
        LiteralValue::Boolean(b) => Type::BooleanLiteral(*b),
    }
}

/// Whether `ty` is, or has a member that is, a literal or `null`: what makes
/// a property a discriminant in TypeScript.
pub fn has_unit_member(ty: &Type) -> bool {
    match ty.peel() {
        Type::Union(members) => members.iter().any(has_unit_member),
        Type::Null => true,
        other => unit_literal_value(other).is_some(),
    }
}

/// Whether every value `ty` holds is one of the `covered` literals.
/// Whether `case` labels for the `covered` literals, and for `null` when
/// `covers_null`, match every value of `ty`.
pub fn is_covered_by_literals(
    ty: &Type,
    covered: &BTreeSet<LiteralValue>,
    covers_null: bool,
) -> bool {
    match ty.peel() {
        Type::Union(members) => members
            .iter()
            .all(|member| is_covered_by_literals(member, covered, covers_null)),
        Type::Null => covers_null,
        // `boolean` is `true | false`.
        Type::Boolean => [true, false]
            .into_iter()
            .all(|value| covered.contains(&LiteralValue::Boolean(value))),
        other => unit_literal_value(other).is_some_and(|value| covered.contains(&value)),
    }
}

/// Whether `ty` is a type parameter, or a union with one.
pub fn has_type_parameter_member(ty: &Type) -> bool {
    match ty.peel() {
        Type::Union(members) => members.iter().any(has_type_parameter_member),
        other => matches!(other, Type::TypeVar(_) | Type::GenericParam { .. }),
    }
}

pub(super) fn unit_literal_value(ty: &Type) -> Option<LiteralValue> {
    match ty.peel() {
        Type::StringLiteral(s) => Some(LiteralValue::String(s.clone())),
        Type::NumberLiteral(n) => Some(LiteralValue::Number(*n)),
        Type::BooleanLiteral(b) => Some(LiteralValue::Boolean(*b)),
        _ => None,
    }
}

/// Strip `Type::Null` from a union. Returns `Type::Error` for `Type::Null`
/// itself (empty union has no representation).
pub fn strip_null(ty: &Type) -> Type {
    if let Type::Refined {
        original,
        ty: shape,
    } = ty.without_aliases()
    {
        return preserve_refinement(original, strip_null(shape));
    }
    match ty.peel() {
        Type::Null => Type::Error,
        Type::Union(members) => {
            let non_null: Vec<Type> = members
                .iter()
                .filter(|m| !matches!(m.peel(), Type::Null))
                .cloned()
                .collect();
            Type::union(non_null)
        }
        // `unknown` may or may not be null. Stripping null
        // leaves `unknown` — we don't statically know the non-null
        // subset. The NE_NULL fact attached by the caller carries the
        // load-bearing information; the narrowed type stays dynamic.
        Type::Unknown => Type::Unknown,
        // `ty` itself, not its peel: peeling drops a `readonly` wrapper.
        _ => ty.clone(),
    }
}

/// `in` retains optional members in both branches: declaring a property does
/// not prove that its value is present. The true branch removes optionality,
/// preserving any null explicitly included in the declared field type.
pub fn narrow_field_presence(
    receiver_ty: &Type,
    field: &str,
    lookup: &dyn Fn(&Type, &str) -> Option<crate::ObjectField>,
) -> (Type, Type) {
    if let Type::Refined {
        original,
        ty: shape,
    } = receiver_ty.without_aliases()
    {
        let (present, absent) = narrow_field_presence(shape, field, lookup);
        return (
            preserve_refinement(original, present),
            preserve_refinement(original, absent),
        );
    }
    match receiver_ty.peel() {
        Type::Unknown => {
            let fields = BTreeMap::from([(
                field.to_string(),
                crate::ObjectField::required(Type::Unknown),
            )]);
            (
                Type::Object {
                    index: None,
                    fields,
                },
                Type::Unknown,
            )
        }
        Type::Object { fields, .. } => {
            let mut present = fields.clone();
            let entry = present
                .entry(field.to_string())
                .or_insert_with(|| crate::ObjectField::required(Type::Unknown));
            entry.optional = false;
            let absent = match fields.get(field) {
                Some(f) if !f.optional => Type::Error,
                _ => receiver_ty.clone(),
            };
            (
                Type::Object {
                    index: None,
                    fields: present,
                },
                absent,
            )
        }
        Type::Union(members) => {
            let has = members
                .iter()
                .filter(|m| lookup(m, field).is_some())
                .cloned()
                .collect();
            let lacks = members
                .iter()
                .filter(|m| lookup(m, field).is_none_or(|f| f.optional))
                .cloned()
                .collect();
            (Type::union(has), Type::union(lacks))
        }
        Type::InterfaceRef { .. } | Type::ClassRef { .. } => {
            let absent = if lookup(receiver_ty, field).is_some_and(|f| !f.optional) {
                Type::Error
            } else {
                receiver_ty.clone()
            };
            (receiver_ty.clone(), absent)
        }
        _ => (Type::Error, Type::Error),
    }
}

/// `BTreeMap`, not `HashMap`, so a walk over an env is reproducible. Under a hash map
/// `enter_closure_narrow_boundary` would hand the same program different `#narrow_<N>`
/// numbering and different `ExprId`s per compile, and `install_joined_narrowings`'s
/// `joined_rebound` and `retain_emittable_views`'s `candidates` would inherit that order.
/// Only the region nesting reaches the emitted Wasm — [`wrap_order`] pins that, and
/// locals are unnamed there — so this is defence in depth for everything else.
#[derive(Clone, Debug, Default)]
pub struct NarrowEnv {
    views: BTreeMap<ReferencePath, NarrowedView>,
    /// Predicate drops belong to this outcome, never to the condition's parent
    /// scope. A consumer that installs no frame simply discards these records.
    pub dropped: BTreeMap<ReferencePath, Type>,
}

impl NarrowEnv {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, path: ReferencePath, view: NarrowedView) -> Option<NarrowedView> {
        self.dropped.remove(&path);
        self.views.insert(path, view)
    }

    pub fn extend_env(&mut self, other: Self) {
        self.dropped.extend(other.dropped);
        self.extend(other.views);
    }

    pub fn clear(&mut self) {
        self.views.clear();
        self.dropped.clear();
    }
}

impl std::ops::Deref for NarrowEnv {
    type Target = BTreeMap<ReferencePath, NarrowedView>;
    fn deref(&self) -> &Self::Target {
        &self.views
    }
}

impl std::ops::DerefMut for NarrowEnv {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.views
    }
}

impl IntoIterator for NarrowEnv {
    type Item = (ReferencePath, NarrowedView);
    type IntoIter = std::collections::btree_map::IntoIter<ReferencePath, NarrowedView>;
    fn into_iter(self) -> Self::IntoIter {
        self.views.into_iter()
    }
}

impl<'a> IntoIterator for &'a NarrowEnv {
    type Item = (&'a ReferencePath, &'a NarrowedView);
    type IntoIter = std::collections::btree_map::Iter<'a, ReferencePath, NarrowedView>;
    fn into_iter(self) -> Self::IntoIter {
        self.views.iter()
    }
}

impl Extend<(ReferencePath, NarrowedView)> for NarrowEnv {
    fn extend<I: IntoIterator<Item = (ReferencePath, NarrowedView)>>(&mut self, iter: I) {
        for (path, view) in iter {
            self.insert(path, view);
        }
    }
}

impl FromIterator<(ReferencePath, NarrowedView)> for NarrowEnv {
    fn from_iter<I: IntoIterator<Item = (ReferencePath, NarrowedView)>>(iter: I) -> Self {
        let mut env = Self::new();
        env.extend(iter);
        env
    }
}

/// Nesting order for the wrap-the-body-in-one-region-per-path loops: longest chain
/// first, so every path is wrapped *inside* the prefixes its source reads. Ties break
/// on the path itself, making this a total order — the emitted nesting must not depend
/// on the order the caller happened to iterate its env in.
pub fn wrap_order(a: &ReferencePath, b: &ReferencePath) -> std::cmp::Ordering {
    b.chain.len().cmp(&a.chain.len()).then_with(|| a.cmp(b))
}

/// Compares only `narrowed_ty` per path — `binding` and `source` differ
/// across mints even when the narrowing is unchanged.
pub fn envs_per_path_narrowed_ty_equal(a: &NarrowEnv, b: &NarrowEnv) -> bool {
    if a.len() != b.len() {
        return false;
    }
    for (path, a_view) in a {
        match b.get(path) {
            Some(b_view) if a_view.narrowed_ty == b_view.narrowed_ty => continue,
            _ => return false,
        }
    }
    true
}

/// JS-truthiness classification of a single (non-union) type: which runtime
/// test decides truthiness for values of that member.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TruthinessClass {
    AlwaysTruthy,
    AlwaysFalsy,
    /// Falsy iff `0`, `-0`, or `NaN`. Includes number enums (may hold `0`).
    NumberLike,
    /// Falsy iff `""`. Includes string enums (may hold `""`).
    StringLike,
    /// Falsy iff `false`.
    BooleanLike,
    /// Falsy iff `0n`.
    BigIntLike,
    /// Erased at compile time — needs the full runtime dispatch.
    Dynamic,
}

pub fn truthiness_class(member: &Type) -> TruthinessClass {
    use TruthinessClass::*;
    match member.peel() {
        Type::Null => AlwaysFalsy,
        Type::Boolean => BooleanLike,
        Type::BooleanLiteral(true) => AlwaysTruthy,
        Type::BooleanLiteral(false) => AlwaysFalsy,
        Type::Number => NumberLike,
        Type::NumberLiteral(n) => {
            if n.0 == 0.0 || n.0.is_nan() {
                AlwaysFalsy
            } else {
                AlwaysTruthy
            }
        }
        Type::String => StringLike,
        Type::StringLiteral(s) => {
            if s.is_empty() {
                AlwaysFalsy
            } else {
                AlwaysTruthy
            }
        }
        Type::BigInt => BigIntLike,
        Type::NumberEnum { .. } => NumberLike,
        Type::StringEnum { .. } => StringLike,
        // `{}` admits every value but `null` and `undefined`, falsy primitives
        // included.
        empty if is_empty_object(empty) => Dynamic,
        Type::Object { .. }
        | Type::Array(_)
        | Type::Tuple(_)
        | Type::Function { .. }
        | Type::Uint8Array
        | Type::InterfaceRef { .. }
        | Type::ClassRef { .. } => AlwaysTruthy,
        _ => Dynamic,
    }
}

/// Whether `ty` is `{}`, the object type with no members.
fn is_empty_object(ty: &Type) -> bool {
    matches!(ty.peel(), Type::Object { fields, index } if fields.is_empty() && index.is_none())
}

/// Whether every member of `ty` is always truthy or always falsy, so a
/// truthiness test that rules a member out rules out every value it holds.
pub fn has_known_truthiness(ty: &Type) -> bool {
    union_members(ty).into_iter().all(|member| {
        matches!(
            truthiness_class(member),
            TruthinessClass::AlwaysTruthy | TruthinessClass::AlwaysFalsy
        )
    })
}

pub(super) fn union_members(ty: &Type) -> Vec<&Type> {
    match ty.peel() {
        Type::Union(members) => members.iter().collect(),
        // `ty` itself, not its peel: peeling drops a `readonly` wrapper.
        _ => vec![ty],
    }
}

/// The type of `x` where `x` is known truthy: drop `null` and falsy literal
/// types per member. Falsy-capable base types stay whole (matching TS, which
/// doesn't invent a "non-empty string" type).
pub fn truthy_part(ty: &Type) -> Type {
    if let Type::Refined {
        original,
        ty: shape,
    } = ty.without_aliases()
    {
        return preserve_refinement(original, truthy_part(shape));
    }
    // `boolean` is exactly `true | false` (spec §1.2), so its truthy part is `true`.
    let kept: Vec<Type> = union_members(ty)
        .into_iter()
        .filter(|m| truthiness_class(m) != TruthinessClass::AlwaysFalsy)
        .map(|m| match m.without_aliases() {
            Type::Boolean => Type::BooleanLiteral(true),
            _ => m.clone(),
        })
        .collect();
    Type::union(kept)
}

/// The type of `x` where `x` is known falsy: keep `null` and falsy literals,
/// collapse `string` to `""` and `boolean` to `false`, drop never-falsy
/// reference types. `number` stays `number` — a `0` literal would be unsound
/// for `NaN`/`-0`.
pub fn falsy_part(ty: &Type) -> Type {
    if let Type::Refined {
        original,
        ty: shape,
    } = ty.without_aliases()
    {
        return preserve_refinement(original, falsy_part(shape));
    }
    let kept: Vec<Type> = union_members(ty)
        .into_iter()
        .filter_map(|m| match truthiness_class(m) {
            TruthinessClass::AlwaysTruthy => None,
            // TypeScript keeps only the definitely falsy part of `{}`, which
            // is nothing; Submilli's `{}` holds only objects so far.
            TruthinessClass::Dynamic if is_empty_object(m) => None,
            TruthinessClass::StringLike => Some(match m.peel() {
                Type::String => Type::StringLiteral(String::new()),
                _ => m.clone(),
            }),
            _ => Some(match m.without_aliases() {
                Type::Boolean => Type::BooleanLiteral(false),
                _ => m.clone(),
            }),
        })
        .collect();
    Type::union(kept)
}

/// True when every falsy value `ty` can hold is `null` — i.e. a truthiness
/// test is exactly a null test, so the false branch may assert `EQ_NULL`.
pub fn falsy_values_are_only_null(ty: &Type) -> bool {
    union_members(ty).into_iter().all(|m| {
        matches!(m.peel(), Type::Null) || truthiness_class(m) == TruthinessClass::AlwaysTruthy
    })
}

pub fn type_matches_facts(ty: &Type, facts: TypeFacts) -> bool {
    // Aliases are transparent for type identity — peel before matching so an
    // aliased primitive/union (e.g. `type NS = number | string`) classifies the
    // same as its inline spelling.
    let ty = ty.peel();
    // TRUTHY/FALSY subsume the null tests (truthy ⇒ non-null), so they are
    // checked first: `TRUTHY | NE_NULL` filters by truthiness, bare `NE_NULL`
    // stays a pure null test.
    if facts.contains(TypeFacts::TRUTHY) {
        return truthiness_class(ty) != TruthinessClass::AlwaysFalsy;
    }
    if facts.contains(TypeFacts::FALSY) {
        return truthiness_class(ty) != TruthinessClass::AlwaysTruthy;
    }
    if facts.contains(TypeFacts::NE_NULL) {
        return !matches!(ty, Type::Null);
    }
    if facts.contains(TypeFacts::EQ_NULL) {
        return matches!(ty, Type::Null);
    }
    if facts.contains(TypeFacts::IS_NUMBER) {
        // An enum member's `typeof` is its underlying primitive, as in TypeScript.
        return matches!(
            ty,
            Type::Number | Type::NumberLiteral(_) | Type::NumberEnum { .. }
        );
    }
    if facts.contains(TypeFacts::IS_STRING) {
        return matches!(
            ty,
            Type::String | Type::StringLiteral(_) | Type::StringEnum { .. }
        );
    }
    if facts.contains(TypeFacts::IS_BOOLEAN) {
        return matches!(ty, Type::Boolean | Type::BooleanLiteral(_));
    }
    if facts.contains(TypeFacts::IS_ARRAY) {
        return matches!(ty, Type::Array(_));
    }
    if facts.contains(TypeFacts::IS_FUNCTION) {
        return matches!(ty, Type::Function { .. });
    }
    if facts.contains(TypeFacts::IS_OBJECT) {
        return is_typeof_object(ty);
    }
    false
}

/// Does `typeof` answer `"object"` for a value of `ty`?
///
/// Exhaustive on purpose. `"object"` is the complement of the primitive tags,
/// so a type constructor with no arm here is a value with *no* `typeof` at all —
/// every one of the five tags folds to `false`, which is unsatisfiable and
/// silent. Matching exhaustively makes a new [`Type`] variant fail to compile
/// until someone decides its tag.
fn is_typeof_object(ty: &Type) -> bool {
    match ty {
        // A union reaches here from the tuple-element rollup, where the
        // question is whether the whole element answers the tag.
        Type::Union(members) => members.iter().all(is_typeof_object),

        Type::Object { .. }
        | Type::Array(_)
        | Type::Tuple(_)
        | Type::InterfaceRef { .. }
        | Type::ClassRef { .. }
        | Type::Uint8Array
        // `typeof null === "object"` — the JS quirk.
        | Type::Null => true,

        Type::Number
        | Type::NumberLiteral(_)
        | Type::NumberEnum { .. }
        | Type::String
        | Type::StringLiteral(_)
        | Type::StringEnum { .. }
        | Type::Boolean
        | Type::BooleanLiteral(_)
        | Type::BigInt
        | Type::Function { .. } => false,

        // Not classifiable, for four different reasons: no value at all
        // (`Void`, `Never`), a diagnostic already reported (`Error`), erased at
        // runtime or bodyless (`TypeVar`, `GenericParam`, `Unknown`, and a
        // recursive alias's back-edge `AliasRef`). `fold_typeof_tag` bails on
        // the erased set before it ever asks, so `false` here only ever means
        // "narrows nothing", never "answers no tag".
        Type::Void
        | Type::Never
        | Type::Error
        | Type::Unknown
        | Type::TypeVar(_)
        | Type::GenericParam { .. }
        | Type::AliasRef { .. } => false,

        // Peeled by the caller.
        Type::Alias { .. } | Type::Refined { .. } | Type::Readonly(_) => false,
    }
}

/// Keep only the union members of `ty` that match `facts`. For a
/// non-union `ty`, returns `ty` if matched or [`Type::Error`] if not.
pub fn intersect_with(ty: &Type, facts: TypeFacts) -> Type {
    if let Type::Refined {
        original,
        ty: shape,
    } = ty.without_aliases()
    {
        return preserve_refinement(original, intersect_with(shape, facts));
    }
    let peeled = ty.peel();
    if let Some(asserted) = asserted_type_for_facts(facts) {
        match peeled {
            Type::Unknown => return asserted,
            Type::GenericParam { .. } => {
                return Type::Refined {
                    original: Box::new(ty.clone()),
                    ty: Box::new(asserted),
                };
            }
            _ => {}
        }
    }
    match peeled {
        Type::Union(members) => Type::union(
            members
                .iter()
                .map(|m| intersect_with(m, facts))
                .filter(|ty| !matches!(ty, Type::Error))
                .collect(),
        ),
        // `boolean` is `true | false`, and truthiness splits it.
        Type::Boolean if facts == TypeFacts::TRUTHY => Type::BooleanLiteral(true),
        Type::Boolean if facts == TypeFacts::FALSY => Type::BooleanLiteral(false),
        _ if type_matches_facts(peeled, facts) => ty.clone(),
        _ if matches!(peeled, Type::Unknown) => Type::Unknown,
        _ => Type::Error,
    }
}

pub fn subtract(ty: &Type, facts: TypeFacts) -> Type {
    if let Type::Refined {
        original,
        ty: shape,
    } = ty.without_aliases()
    {
        return preserve_refinement(original, subtract(shape, facts));
    }
    let peeled = ty.peel();
    // false-branch on `unknown` is still `unknown` — we can't pin
    // down "unknown minus T" statically.
    if matches!(peeled, Type::Unknown) {
        return Type::Unknown;
    }
    match peeled {
        Type::Union(members) => {
            let kept: Vec<Type> = members
                .iter()
                .filter(|m| !type_matches_facts(m, facts))
                .cloned()
                .collect();
            Type::union(kept)
        }
        _ if !type_matches_facts(peeled, facts) => ty.clone(),
        _ => Type::Error,
    }
}

pub(super) fn with_source_refinement(source: &Type, shape: Type) -> Type {
    match source.without_aliases() {
        Type::Refined { original, .. } => preserve_refinement(original, shape),
        _ => shape,
    }
}

pub(super) fn preserve_refinement(original: &Type, shape: Type) -> Type {
    if matches!(shape, Type::Error | Type::Never) {
        return shape;
    }
    Type::Refined {
        original: Box::new(original.clone()),
        ty: Box::new(shape),
    }
}

/// Strip covered literal values from `ty`. When the residual is `Type::Never`,
/// all discriminant values were covered by `case` labels.
///
/// - **Union**: filter out covered literal members, keeping the rest (`number`
///   in `number | "a"`); collapses to `Never` when all drop.
/// - **Single literal**: `Never` if covered, unchanged otherwise.
/// - **Anything else**: returned unchanged.
pub fn subtract_literals(ty: &Type, covered: &BTreeSet<LiteralValue>) -> Type {
    if let Type::Refined {
        original,
        ty: shape,
    } = ty.without_aliases()
    {
        return preserve_refinement(original, subtract_literals(shape, covered));
    }
    match ty.peel() {
        Type::Union(members) => {
            let kept: Vec<Type> = members
                .iter()
                .map(|m| subtract_literals(m, covered))
                .collect();
            Type::union(kept)
        }
        // `boolean` is `true | false`, so `case true:` leaves `false`.
        Type::Boolean => {
            let remaining: Vec<Type> = [false, true]
                .into_iter()
                .filter(|b| !covered.contains(&LiteralValue::Boolean(*b)))
                .map(Type::BooleanLiteral)
                .collect();
            Type::union(remaining)
        }
        single @ (Type::StringLiteral(_) | Type::NumberLiteral(_) | Type::BooleanLiteral(_)) => {
            match unit_literal_value(single) {
                Some(lit) if covered.contains(&lit) => Type::Never,
                _ => ty.clone(),
            }
        }
        _ => ty.clone(),
    }
}

/// Map a [`TypeFacts`] mask to the concrete `Type` it asserts when narrowing
/// `unknown`. Returns `None` when facts don't pin a single type (TRUTHY,
/// IS_OBJECT span multiple types).
fn asserted_type_for_facts(facts: TypeFacts) -> Option<Type> {
    if facts.contains(TypeFacts::IS_NUMBER) {
        return Some(Type::Number);
    }
    if facts.contains(TypeFacts::IS_STRING) {
        return Some(Type::String);
    }
    if facts.contains(TypeFacts::IS_BOOLEAN) {
        return Some(Type::Boolean);
    }
    if facts.contains(TypeFacts::IS_ARRAY) {
        return Some(Type::Array(Box::new(Type::Unknown)));
    }
    if facts.contains(TypeFacts::EQ_NULL) {
        return Some(Type::Null);
    }
    // IS_OBJECT admits arrays, objects, and null (the JS typeof-null quirk);
    // no single concrete type, so caller keeps `Unknown`.
    None
}

/// Join two branch post-environments at a control-flow merge point:
///
/// - Paths narrowed in **both** branches: joined type is `Type::union` of the two.
/// - Paths narrowed in only one branch: dropped.
/// - Paths a branch *extended* by assignment (`o` assigned, `o.f` narrowed):
///   dropped — the write can invalidate everything below it.
///
/// An assignment to the narrowed path *itself* does not drop it: each branch's
/// env already carries what that branch's assignment left behind
/// (`install_assignment_narrowing` records the assigned type), so the union of
/// the two is exactly the post-join type. That is what makes
/// `if (s === null) { s = "x"; }` leave `s: string` behind.
///
/// Assumes both branches contributed; caller handles the unreachable-branch case.
pub fn union_envs(
    a_narrowings: NarrowEnv,
    a_assigned: BTreeSet<ReferencePath>,
    b_narrowings: NarrowEnv,
    b_assigned: BTreeSet<ReferencePath>,
) -> (NarrowEnv, BTreeSet<ReferencePath>) {
    let mut joined_narrowings = NarrowEnv::new();
    for (path, a_view) in &a_narrowings {
        if let Some(b_view) = b_narrowings.get(path) {
            let joined_ty = join_flow_types(&a_view.narrowed_ty, &b_view.narrowed_ty);
            // A ruled-out view has no shadow, so the join reads through the other.
            let a_view = if is_ruled_out(&a_view.narrowed_ty) {
                b_view
            } else {
                a_view
            };
            joined_narrowings.insert(
                path.clone(),
                NarrowedView {
                    narrowed_ty: joined_ty,
                    // Facts are per-branch; the join can't preserve
                    // them precisely. The downstream slim-slice
                    // sites only consume `narrowed_ty`, so EMPTY is
                    // sound.
                    facts: TypeFacts::EMPTY,
                    excluded_literals: BTreeSet::new(),
                    // Reuse `a`'s binding + source. The join
                    // typically gets dropped by the assigned-paths
                    // filter below; if it survives, the binding /
                    // source remain valid since both branches saw
                    // the same path.
                    binding: a_view.binding.clone(),
                    source: a_view.source,
                },
            );
        }
    }

    for (path, ty) in a_narrowings.dropped.iter().chain(&b_narrowings.dropped) {
        let left_ty = a_narrowings
            .dropped
            .get(path)
            .or_else(|| a_narrowings.get(path).map(|view| &view.narrowed_ty));
        let right_ty = b_narrowings
            .dropped
            .get(path)
            .or_else(|| b_narrowings.get(path).map(|view| &view.narrowed_ty));
        if left_ty == Some(ty) && right_ty == Some(ty) {
            joined_narrowings.dropped.insert(path.clone(), ty.clone());
        }
    }

    let mut joined_assigned = a_assigned;
    joined_assigned.extend(b_assigned);

    joined_narrowings.dropped.retain(|key, _| {
        !joined_assigned
            .iter()
            .any(|assigned| assigned.is_prefix_of(key))
    });
    joined_narrowings.retain(|key, _| {
        !joined_assigned
            .iter()
            .any(|assigned| assigned != key && assigned.is_prefix_of(key))
    });

    (joined_narrowings, joined_assigned)
}

/// The tuple position a number names, if it names one.
pub(super) fn tuple_position(index: f64) -> Option<usize> {
    (index >= 0.0 && index.fract() == 0.0 && index <= u32::MAX as f64).then_some(index as usize)
}

/// The type of a view whose guard ruled out every value. Narrowing yields
/// `Error` when nothing is left; a view keeps it, rather than `never`, so
/// codegen gives the path no shadow local, and a read of the path turns it
/// into `never` where `Inferer::rules_out_to_never` allows.
pub const RULED_OUT: Type = Type::Error;

/// Whether a narrowed type is [`RULED_OUT`].
pub fn is_ruled_out(ty: &Type) -> bool {
    matches!(ty, Type::Error)
}

/// Flow joins collapse a literal already covered by a broad primitive. Keep
/// authored unions unchanged: their overlap is meaningful to JSON diagnostics.
pub(super) fn join_flow_types(left: &Type, right: &Type) -> Type {
    // A ruled-out side holds no value, so it adds nothing to the other.
    if is_ruled_out(left) {
        return right.clone();
    }
    if is_ruled_out(right) {
        return left.clone();
    }
    let joined = Type::union(vec![left.clone(), right.clone()]);
    let Type::Union(mut members) = joined else {
        return joined;
    };
    let has_number = members
        .iter()
        .any(|ty| matches!(ty.without_aliases(), Type::Number));
    let has_string = members
        .iter()
        .any(|ty| matches!(ty.without_aliases(), Type::String));
    members.retain(|ty| match ty.without_aliases() {
        Type::NumberLiteral(_) => !has_number,
        Type::StringLiteral(_) => !has_string,
        _ => true,
    });
    Type::union(members)
}

/// Recurses into `Type::Union` members only — composite slots have fixed runtime
/// classification regardless of generic contents, so their type arguments are
/// not walked either (`Box<T> = number` classifies as a number whatever `T` is).
///
/// Peeled: an alias of a type parameter is erased just as the parameter is, and
/// the caller uses this as the escape hatch that keeps a `typeof` guard on an
/// erased operand from folding to a constant.
pub fn has_erased_member(ty: &Type) -> bool {
    match ty.peel() {
        Type::TypeVar(_) | Type::GenericParam { .. } => true,
        Type::Union(members) => members.iter().any(has_erased_member),
        _ => false,
    }
}

/// Every type is condition-compatible under JS truthiness except `unknown`
/// (requires explicit narrowing first, spec §2.11) and the value-less
/// `void`/`never`. `Error` is accepted to silence cascades.
pub fn condition_compatible(ty: &Type) -> bool {
    // `carries_void` rather than a bare `Void` match: a union with a `void`
    // member has no more of a runtime value than bare `void` does.
    !ty.carries_void() && !matches!(ty.peel(), Type::Unknown | Type::Never)
}

pub fn facts_for_target_type(target: &Type) -> TypeFacts {
    match target.peel() {
        Type::Number | Type::NumberLiteral(_) => TypeFacts::IS_NUMBER,
        Type::String | Type::StringLiteral(_) => TypeFacts::IS_STRING,
        Type::Boolean | Type::BooleanLiteral(_) => TypeFacts::IS_BOOLEAN,
        Type::Null => TypeFacts::EQ_NULL,
        Type::Array(_) => TypeFacts::IS_ARRAY,
        _ => TypeFacts::EMPTY,
    }
}

#[cfg(test)]
mod analyzer_tests {
    use super::*;

    #[test]
    fn refined_literal_residual_retains_generic_identity() {
        let original = Type::GenericParam {
            id: 1,
            name: "T".into(),
        };
        let shape = Type::union(vec![
            Type::StringLiteral("a".into()),
            Type::StringLiteral("b".into()),
        ]);
        let refined = preserve_refinement(&original, shape);
        let covered = BTreeSet::from([LiteralValue::String("a".into())]);
        assert_eq!(
            subtract_literals(&refined, &covered),
            preserve_refinement(&original, Type::StringLiteral("b".into()))
        );
    }

    #[test]
    fn strip_null_from_nullable_string() {
        let ty = Type::union(vec![Type::String, Type::Null]);
        assert_eq!(strip_null(&ty), Type::String);
    }

    #[test]
    fn strip_null_from_non_nullable_is_identity() {
        assert_eq!(strip_null(&Type::String), Type::String);
        assert_eq!(strip_null(&Type::Number), Type::Number);
    }

    #[test]
    fn strip_null_from_null_is_error() {
        assert_eq!(strip_null(&Type::Null), Type::Error);
    }

    #[test]
    fn strip_null_from_multi_member_union() {
        let ty = Type::union(vec![Type::String, Type::Number, Type::Null]);
        let stripped = strip_null(&ty);
        // Type::union canonicalizes; should be String | Number.
        match stripped {
            Type::Union(members) => assert_eq!(members.len(), 2),
            _ => panic!("expected Union, got {stripped:?}"),
        }
    }

    #[test]
    fn intersect_with_is_number_filters_union() {
        let ty = Type::union(vec![Type::Number, Type::String, Type::Boolean]);
        let kept = intersect_with(&ty, TypeFacts::IS_NUMBER);
        assert_eq!(kept, Type::Number);
    }

    #[test]
    fn subtract_is_number_keeps_others() {
        let ty = Type::union(vec![Type::Number, Type::String, Type::Boolean]);
        let kept = subtract(&ty, TypeFacts::IS_NUMBER);
        match kept {
            Type::Union(members) => {
                assert_eq!(members.len(), 2);
                assert!(members.contains(&Type::String));
                assert!(members.contains(&Type::Boolean));
            }
            _ => panic!("expected Union, got {kept:?}"),
        }
    }

    #[test]
    fn intersect_with_truthy_strips_null() {
        let ty = Type::union(vec![Type::String, Type::Null]);
        let kept = intersect_with(&ty, TypeFacts::TRUTHY);
        assert_eq!(kept, Type::String);
    }

    #[test]
    fn subtract_truthy_keeps_only_null() {
        let ty = Type::union(vec![Type::String, Type::Null]);
        let kept = subtract(&ty, TypeFacts::TRUTHY);
        assert_eq!(kept, Type::Null);
    }

    #[test]
    fn intersect_with_no_match_yields_error() {
        let kept = intersect_with(&Type::String, TypeFacts::IS_NUMBER);
        assert_eq!(kept, Type::Error);
    }

    #[test]
    fn facts_for_target_type_covers_primitives() {
        assert_eq!(facts_for_target_type(&Type::Number), TypeFacts::IS_NUMBER);
        assert_eq!(facts_for_target_type(&Type::String), TypeFacts::IS_STRING);
        assert_eq!(facts_for_target_type(&Type::Boolean), TypeFacts::IS_BOOLEAN);
        assert_eq!(facts_for_target_type(&Type::Null), TypeFacts::EQ_NULL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Span;

    #[test]
    fn reference_path_equality_is_structural() {
        let a = ReferencePath::root(BindingId::Local {
            name: "x".to_string(),
            decl_scope: ScopeId(1),
        });
        let b = ReferencePath::root(BindingId::Local {
            name: "x".to_string(),
            decl_scope: ScopeId(1),
        });
        assert_eq!(a, b);
    }

    #[test]
    fn reference_path_distinguishes_scopes() {
        let a = ReferencePath::root(BindingId::Local {
            name: "x".to_string(),
            decl_scope: ScopeId(1),
        });
        let b = ReferencePath::root(BindingId::Local {
            name: "x".to_string(),
            decl_scope: ScopeId(2),
        });
        assert_ne!(a, b);
    }

    #[test]
    fn reference_path_distinguishes_chain() {
        let a = ReferencePath {
            root: BindingId::Local {
                name: "x".to_string(),
                decl_scope: ScopeId(1),
            },
            chain: vec![PathElem::Field("foo".to_string())],
        };
        let b = ReferencePath::root(BindingId::Local {
            name: "x".to_string(),
            decl_scope: ScopeId(1),
        });
        assert_ne!(a, b);
    }

    #[test]
    fn type_facts_or_combines() {
        let f = TypeFacts::TRUTHY | TypeFacts::NE_NULL;
        assert!(f.contains(TypeFacts::TRUTHY));
        assert!(f.contains(TypeFacts::NE_NULL));
        assert!(!f.contains(TypeFacts::EQ_NULL));
    }

    #[test]
    fn type_facts_not_complements() {
        let f = TypeFacts::EQ_NULL;
        let flipped = !f;
        assert!(flipped.contains(TypeFacts::NE_NULL));
        assert!(!flipped.contains(TypeFacts::EQ_NULL));
    }

    #[test]
    fn binding_id_global_uses_mangled_name() {
        let g = BindingId::Global(crate::mangle::package_symbol("main", "x"));
        if let BindingId::Global(name) = &g {
            assert_eq!(name.as_str(), "main#x");
        } else {
            panic!("expected Global");
        }
    }

    #[test]
    fn narrowed_view_construction() {
        let view = NarrowedView {
            narrowed_ty: Type::String,
            facts: TypeFacts::NE_NULL,
            excluded_literals: BTreeSet::new(),
            binding: Ident {
                name: "#narrow_0".to_string(),
                span: Span::new(crate::FileId(0), 0, 1).unwrap(),
            },
            source: crate::ExprId(0),
        };
        assert_eq!(view.narrowed_ty, Type::String);
        assert!(view.facts.contains(TypeFacts::NE_NULL));
    }

    #[test]
    fn cast_info_holds_kind_and_types() {
        let info = CastInfo {
            from_ty: Type::Union(vec![Type::String, Type::Null]),
            to_ty: Type::String,
            cast_kind: CastKind::NonNull,
        };
        assert!(matches!(info.cast_kind, CastKind::NonNull));
    }

    #[test]
    fn union_discriminant_stub_returns_none() {
        assert!(union_discriminant(&[Type::String, Type::Null]).is_none());
    }

    #[test]
    fn intersect_unknown_with_is_string() {
        assert_eq!(
            intersect_with(&Type::Unknown, TypeFacts::IS_STRING),
            Type::String
        );
    }

    #[test]
    fn intersect_unknown_with_is_number() {
        assert_eq!(
            intersect_with(&Type::Unknown, TypeFacts::IS_NUMBER),
            Type::Number
        );
    }

    #[test]
    fn intersect_unknown_with_is_boolean() {
        assert_eq!(
            intersect_with(&Type::Unknown, TypeFacts::IS_BOOLEAN),
            Type::Boolean
        );
    }

    #[test]
    fn intersect_unknown_with_is_array_carries_unknown_element() {
        // `Array.isArray<T>(unknown)` narrows to `Array<unknown>`
        // because the element type isn't statically determined.
        assert_eq!(
            intersect_with(&Type::Unknown, TypeFacts::IS_ARRAY),
            Type::Array(Box::new(Type::Unknown)),
        );
    }

    #[test]
    fn intersect_unknown_with_eq_null() {
        assert_eq!(
            intersect_with(&Type::Unknown, TypeFacts::EQ_NULL),
            Type::Null
        );
    }

    #[test]
    fn intersect_unknown_with_truthy_stays_unknown() {
        // Truthiness doesn't pin a concrete type — unknown stays
        // unknown (with the TRUTHY/NE_NULL fact carried on the view).
        assert_eq!(
            intersect_with(&Type::Unknown, TypeFacts::TRUTHY),
            Type::Unknown
        );
    }

    #[test]
    fn subtract_unknown_stays_unknown() {
        // False-branch of any predicate on unknown is still unknown —
        // we can't statically pin down "unknown minus T".
        assert_eq!(
            subtract(&Type::Unknown, TypeFacts::IS_STRING),
            Type::Unknown
        );
        assert_eq!(subtract(&Type::Unknown, TypeFacts::EQ_NULL), Type::Unknown);
        assert_eq!(subtract(&Type::Unknown, TypeFacts::TRUTHY), Type::Unknown);
    }

    #[test]
    fn strip_null_unknown_preserves_unknown() {
        assert_eq!(strip_null(&Type::Unknown), Type::Unknown);
    }

    #[test]
    fn condition_compatible_rejects_unknown() {
        assert!(!condition_compatible(&Type::Unknown));
    }

    #[test]
    fn condition_compatible_accepts_js_truthiness_types() {
        for ty in [
            Type::Boolean,
            Type::String,
            Type::Number,
            Type::BigInt,
            Type::Null,
            Type::Array(Box::new(Type::Number)),
            Type::union(vec![Type::String, Type::Null]),
        ] {
            assert!(condition_compatible(&ty), "{ty:?}");
        }
        assert!(!condition_compatible(&Type::Void));
        assert!(!condition_compatible(&Type::Never));
    }

    #[test]
    fn truthiness_class_table() {
        use TruthinessClass::*;
        let cases = [
            (Type::Null, AlwaysFalsy),
            (Type::Boolean, BooleanLike),
            (Type::Number, NumberLike),
            (
                Type::NumberLiteral(crate::types::LiteralF64(0.0)),
                AlwaysFalsy,
            ),
            (
                Type::NumberLiteral(crate::types::LiteralF64(-0.0)),
                AlwaysFalsy,
            ),
            (
                Type::NumberLiteral(crate::types::LiteralF64(3.0)),
                AlwaysTruthy,
            ),
            (Type::String, StringLike),
            (Type::StringLiteral(String::new()), AlwaysFalsy),
            (Type::StringLiteral("a".to_string()), AlwaysTruthy),
            (Type::BigInt, BigIntLike),
            (Type::Uint8Array, AlwaysTruthy),
            (Type::Array(Box::new(Type::Number)), AlwaysTruthy),
            (Type::Unknown, Dynamic),
        ];
        for (ty, expected) in cases {
            assert_eq!(truthiness_class(&ty), expected, "{ty:?}");
        }
    }

    #[test]
    fn truthy_part_strips_null_and_falsy_literals() {
        assert_eq!(
            truthy_part(&Type::union(vec![Type::String, Type::Null])),
            Type::String
        );
        assert_eq!(
            truthy_part(&Type::union(vec![
                Type::StringLiteral(String::new()),
                Type::StringLiteral("a".to_string()),
                Type::Null,
            ])),
            Type::StringLiteral("a".to_string())
        );
        assert_eq!(truthy_part(&Type::Null), Type::Never);
        assert_eq!(truthy_part(&Type::Boolean), Type::BooleanLiteral(true));
        assert_eq!(truthy_part(&Type::Number), Type::Number);
    }

    #[test]
    fn falsy_part_keeps_falsy_capable_members() {
        assert_eq!(
            falsy_part(&Type::union(vec![Type::String, Type::Null])),
            Type::union(vec![Type::StringLiteral(String::new()), Type::Null])
        );
        assert_eq!(falsy_part(&Type::Number), Type::Number);
        assert_eq!(falsy_part(&Type::Boolean), Type::BooleanLiteral(false));
        // Never-falsy references drop entirely.
        assert_eq!(
            falsy_part(&Type::union(vec![
                Type::Array(Box::new(Type::Number)),
                Type::Null
            ])),
            Type::Null
        );
        assert_eq!(
            falsy_part(&Type::Array(Box::new(Type::Number))),
            Type::Never
        );
    }

    #[test]
    fn falsy_only_null_distinguishes_reference_unions() {
        assert!(falsy_values_are_only_null(&Type::union(vec![
            Type::Array(Box::new(Type::Number)),
            Type::Null
        ])));
        assert!(!falsy_values_are_only_null(&Type::union(vec![
            Type::String,
            Type::Null
        ])));
        assert!(falsy_values_are_only_null(&Type::Uint8Array));
        assert!(!falsy_values_are_only_null(&Type::Number));
    }

    #[test]
    fn intersect_truthy_and_falsy_respect_js_truthiness() {
        let string_or_null = Type::union(vec![Type::String, Type::Null]);
        assert_eq!(
            intersect_with(&string_or_null, TypeFacts::TRUTHY | TypeFacts::NE_NULL),
            Type::String
        );
        // `""` is falsy but not null — the falsy side keeps `string`.
        assert_eq!(
            intersect_with(&string_or_null, TypeFacts::FALSY | TypeFacts::EQ_NULL),
            string_or_null
        );
        let arr_or_null = Type::union(vec![Type::Array(Box::new(Type::Number)), Type::Null]);
        assert_eq!(
            intersect_with(&arr_or_null, TypeFacts::FALSY | TypeFacts::EQ_NULL),
            Type::Null
        );
        // Bare NE_NULL stays a pure null test: `x !== null` keeps `""`.
        assert_eq!(
            intersect_with(&string_or_null, TypeFacts::NE_NULL),
            Type::String
        );
    }
}
