use std::collections::BTreeMap;
use std::fmt;

use crate::mangle::MangledName;
use serde::{Deserialize, Serialize};

/// `f64` wrapper for total ordering and bit-pattern equality — bare `f64` lacks `Eq`/`Ord`,
/// which would break the derived impls on [`Type`]. Construction must canonicalize `-0.0 → 0.0`;
/// NaN can't appear from source literals so bit-pattern equality is safe.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct LiteralF64(#[serde(with = "crate::artifact_f64")] pub f64);

impl PartialEq for LiteralF64 {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}

impl Eq for LiteralF64 {}

impl PartialOrd for LiteralF64 {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for LiteralF64 {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl std::hash::Hash for LiteralF64 {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}

/// Owning package of a by-name [`Type`] reference (`InterfaceRef`, `AliasRef`, `Alias`,
/// `NumberEnum`, `StringEnum`). Carried so *structural* resolution of an already-typed
/// value can find the symbol by FQN in a global registry, independent of what the current
/// module imported.
///
/// **Excluded from identity:** `Eq`/`Ord`/`Hash` treat every `Package` as equal, so the
/// owning package never drives a by-name type's identity. Identity is the sibling
/// `mangled` field (the declaring symbol's mangled name, which already encodes the
/// package and module); `package` only keeps *structural* registry lookup robust to
/// which construction site stamped it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Package(pub String);

impl Package {
    pub fn prelude() -> Self {
        Self(crate::mangle::PRELUDE_PACKAGE.to_string())
    }

    pub fn user() -> Self {
        Self(crate::mangle::USER_PACKAGE.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl PartialEq for Package {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Package {}

impl PartialOrd for Package {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Package {
    fn cmp(&self, _: &Self) -> std::cmp::Ordering {
        std::cmp::Ordering::Equal
    }
}

impl std::hash::Hash for Package {
    fn hash<H: std::hash::Hasher>(&self, _: &mut H) {}
}

/// `optional: true` means reads widen to `ty | null` and construction may omit the field.
/// Distinct from a value-nullable field (`ty: T | null, optional: false`): must be present
/// but can be null.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ObjectField {
    pub ty: Type,
    pub optional: bool,
    /// `readonly` forbids writes through the field; assignability treats a writable
    /// target field invariantly (see `assignable.rs`). Inferred object literals and
    /// freshly-synthesized shapes are writable (`false`); only an explicit `readonly`
    /// modifier on an object-type/interface property sets this.
    pub readonly: bool,
}

impl ObjectField {
    pub fn required(ty: Type) -> Self {
        Self {
            ty,
            optional: false,
            readonly: false,
        }
    }
    pub fn optional(ty: Type) -> Self {
        Self {
            ty,
            optional: true,
            readonly: false,
        }
    }

    /// The type a *read* of this field yields: an optional field widens to
    /// `T | null`, since absence at construction is null at read. Every field
    /// read — object type, interface property, class field, union member, and
    /// codegen's mirror of all four — goes through this, so the widening rule
    /// has one definition.
    pub fn read_ty(&self) -> Type {
        Self::widen_optional(self.optional, self.ty.clone())
    }

    /// [`read_ty`](Self::read_ty) for callers holding the two facts separately
    /// (an interface `PropertySig`, a class `FieldSig`).
    pub fn widen_optional(optional: bool, ty: Type) -> Type {
        if optional {
            Type::union(vec![ty, Type::Null])
        } else {
            ty
        }
    }
}

/// Values available under arbitrary string property names.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct IndexSignature {
    pub value: Box<Type>,
    pub readonly: bool,
}

impl IndexSignature {
    pub fn map_value(&self, transform: impl FnOnce(&Type) -> Type) -> Self {
        Self {
            value: Box::new(transform(&self.value)),
            readonly: self.readonly,
        }
    }

    /// [`map_value`](Self::map_value) with a transform that can fail.
    pub fn try_map_value<E>(
        &self,
        transform: impl FnOnce(&Type) -> Result<Type, E>,
    ) -> Result<Self, E> {
        Ok(Self {
            value: Box::new(transform(&self.value)?),
            readonly: self.readonly,
        })
    }

    pub fn read_ty(&self) -> Type {
        Type::union(vec![(*self.value).clone(), Type::Null])
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TypePredicate {
    pub parameter_index: u32,
    pub asserted_type: Type,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Type {
    Number,
    /// No distinct runtime representation — codegen widens to `f64` at every emission site.
    NumberLiteral(LiteralF64),
    BigInt,
    String,
    StringLiteral(String),
    Uint8Array,
    Boolean,
    /// `true` or `false`. Like the other literal types it has no runtime
    /// representation of its own: it lowers exactly as `boolean`. `Type::union`
    /// folds `true | false` into `boolean`, which is what `boolean` means.
    BooleanLiteral(bool),
    Null,
    Void,
    /// Unlike TypeScript's `any`, requires explicit narrowing before use.
    /// `Unknown | T` collapses to `Unknown`.
    Unknown,
    Function {
        params: Vec<Type>,
        ret: Box<Type>,
        /// Boxed to break the `Type → Function → TypePredicate → Type` cycle. Guards are
        /// assignable to plain functions but not vice versa.
        predicate: Option<Box<TypePredicate>>,
        /// When true, the last `params` entry holds the rest array type.
        has_rest: bool,
    },
    /// `BTreeMap` gives structural `==`, deterministic iteration, and canonical field order for codegen.
    Object {
        fields: BTreeMap<String, ObjectField>,
        index: Option<IndexSignature>,
    },
    Array(Box<Type>),
    /// Parser rejects empty tuples. Index access requires an integer literal; out-of-range and
    /// non-literal indices are rejected at typecheck time. Lowers to `(ref $Array)` at runtime.
    Tuple(Vec<Type>),
    /// `readonly T[]` (also spelled `ReadonlyArray<T>`) or `readonly [A, B]`. The inner
    /// type is always a [`Type::Array`] or [`Type::Tuple`]: the wrapper only forbids
    /// writes, so [`Type::peel`] strips it and every read path sees the plain array.
    /// Write paths (index assignment, mutating methods) must ask
    /// [`Type::is_readonly_array`] before peeling. `Eq`/`Ord` keep it distinct from the
    /// mutable type because a readonly array is not assignable to a mutable one.
    Readonly(Box<Type>),
    /// Diagnostic already reported; downstream code must not emit cascading errors.
    Error,
    /// No Wasm representation — never values never reach codegen.
    Never,
    /// Signature-form generic parameter. Never appears in function bodies — body inference
    /// replaces each TypeVar with a fresh [`GenericParam`](Type::GenericParam) at body entry.
    TypeVar(String),
    /// Body-internal generic placeholder. Identity by `id` — two GPs with different ids are
    /// distinct even when they share a name, preventing collisions across nested generic scopes.
    GenericParam {
        id: u32,
        name: String,
    },
    /// A generic value with a proven runtime shape. Member access and lowering
    /// use `ty`; assignability also preserves the original generic identity.
    Refined {
        original: Box<Type>,
        ty: Box<Type>,
    },
    /// `args` carries already-substituted type arguments; empty for non-generic interfaces.
    /// `mangled` is the declaring symbol's mangled name — the **nominal identity** (two
    /// modules' same-named interfaces have distinct mangled names). `package`/`name` are
    /// kept for diagnostics and import-independent *structural* resolution (see [`Package`]);
    /// they do not drive identity. `mangled` is listed first so derived `Ord` keys on it.
    InterfaceRef {
        mangled: MangledName,
        package: Package,
        name: String,
        args: Vec<Type>,
    },
    /// Nominal reference to a `class` declaration. Identity is `mangled` (two modules'
    /// same-named classes are distinct); `package`/`name` are for diagnostics and
    /// import-independent structural resolution, mirroring [`Type::InterfaceRef`].
    /// `args` instantiates the class's type parameters; empty for a
    /// non-generic class.
    ClassRef {
        mangled: MangledName,
        package: Package,
        name: String,
        args: Vec<Type>,
    },
    /// Distinct from `StringEnum` so codegen knows the `i32` representation without consulting the type namespace.
    NumberEnum {
        mangled: MangledName,
        package: Package,
        name: String,
    },
    StringEnum {
        mangled: MangledName,
        package: Package,
        name: String,
    },
    /// Canonical union: sorted, deduplicated, no nested unions, no `Error` members, always ≥2
    /// members. Build *only* via [`Type::union`].
    Union(Vec<Type>),
    /// Every pattern-match site must call [`Type::peel`] first; sole exception is
    /// [`Display`](fmt::Display). `Eq`/`Ord`/`Hash` are nominal: `Alias { "ID", Number }` ≠
    /// `Number` at the raw level. `args` carries already-substituted type arguments.
    Alias {
        mangled: MangledName,
        package: Package,
        name: String,
        args: Vec<Type>,
        ty: Box<Type>,
    },
    /// Lazy by-name reference to a type alias, used **only at a
    /// recursion back-edge** — the inner `Json` in
    /// `type Json = number | string | Json[]`, or the `Node` in
    /// `type Node = { next: Node | null }`. Unlike [`Type::Alias`] it
    /// carries no inline body; the body lives in the type namespace and
    /// is resolved by name on demand (mirroring [`Type::InterfaceRef`]).
    /// This keeps a recursive alias a *finite* `Type` value — a cyclic
    /// inline body would infinitely recurse the derived `Eq`/`Ord` and
    /// [`Type::peel`]. `peel` does **not** expand it (it stops here, the
    /// same as `InterfaceRef`); the few sites that need the alias's
    /// structure resolve it by name with cycle-safety.
    AliasRef {
        mangled: MangledName,
        package: Package,
        name: String,
        args: Vec<Type>,
    },
}

impl Type {
    /// A receiver whose computed string keys use the object property carrier.
    pub fn is_structural_object(&self) -> bool {
        match self.peel() {
            Self::Object { .. } | Self::InterfaceRef { .. } => true,
            Self::Union(members) => members.iter().all(Self::is_structural_object),
            _ => false,
        }
    }

    /// Build an [`Type::InterfaceRef`]. `mangled` is the declaring symbol's mangled
    /// name (its nominal identity) — pass the symbol's own `mangled_name`, never a
    /// value recomputed from `package`/`name`, so two construction sites for the same
    /// type always agree.
    pub fn interface_ref(
        package: Package,
        name: impl Into<String>,
        mangled: MangledName,
        args: Vec<Type>,
    ) -> Type {
        Type::InterfaceRef {
            mangled,
            package,
            name: name.into(),
            args,
        }
    }

    /// Build a [`Type::ClassRef`]. `mangled` is the class symbol's nominal identity;
    /// pass the symbol's own `mangled_name`, never one recomputed from `package`/`name`.
    pub fn class_ref(
        package: Package,
        name: impl Into<String>,
        mangled: MangledName,
        args: Vec<Type>,
    ) -> Type {
        Type::ClassRef {
            mangled,
            package,
            name: name.into(),
            args,
        }
    }

    pub fn number_enum(package: Package, name: impl Into<String>, mangled: MangledName) -> Type {
        Type::NumberEnum {
            mangled,
            package,
            name: name.into(),
        }
    }

    pub fn string_enum(package: Package, name: impl Into<String>, mangled: MangledName) -> Type {
        Type::StringEnum {
            mangled,
            package,
            name: name.into(),
        }
    }

    pub fn alias_ref(
        package: Package,
        name: impl Into<String>,
        mangled: MangledName,
        args: Vec<Type>,
    ) -> Type {
        Type::AliasRef {
            mangled,
            package,
            name: name.into(),
            args,
        }
    }

    pub fn alias_ty(
        package: Package,
        name: impl Into<String>,
        mangled: MangledName,
        args: Vec<Type>,
        ty: Box<Type>,
    ) -> Type {
        Type::Alias {
            mangled,
            package,
            name: name.into(),
            args,
            ty,
        }
    }

    /// Convenience for a non-namespaced prelude interface (`Error`, `Response`, …):
    /// the prelude registers these with `mangled_name = prelude(name)`, so this is
    /// the matching identity. Namespaced prelude types (`Temporal.*`) must instead
    /// use [`Type::interface_ref`] with `extend(prelude("Temporal"), local)`.
    pub fn prelude_interface(name: impl Into<String>, args: Vec<Type>) -> Type {
        let name = name.into();
        let mangled = crate::mangle::prelude(&name);
        Type::interface_ref(Package::prelude(), name, mangled, args)
    }

    /// The built-in `Error` class reference — the type of `throw`/`catch` values
    /// and the root of user error subclasses.
    pub fn prelude_error_class() -> Type {
        Type::class_ref(
            Package::prelude(),
            "Error",
            crate::mangle::prelude("Error"),
            Vec::new(),
        )
    }

    /// The built-in `RangeError` class reference — the host-implemented
    /// `Error` subclass for out-of-range failures.
    pub fn prelude_range_error_class() -> Type {
        Type::class_ref(
            Package::prelude(),
            "RangeError",
            crate::mangle::prelude("RangeError"),
            Vec::new(),
        )
    }

    /// Remove display aliases while retaining generic guard identity.
    pub fn without_aliases(&self) -> &Type {
        match self {
            Type::Alias { ty, .. } => ty.without_aliases(),
            _ => self,
        }
    }

    pub fn peel(&self) -> &Type {
        let mut t = self;
        while let Type::Alias { ty, .. } | Type::Refined { ty, .. } | Type::Readonly(ty) = t {
            t = ty;
        }
        t
    }

    /// [`peel`](Self::peel), but stopping at a [`Type::Readonly`] wrapper, for
    /// sites that carry a type onward and must not drop its readonly-ness.
    pub fn peel_preserving_readonly(&self) -> &Type {
        let mut t = self;
        while let Type::Alias { ty, .. } | Type::Refined { ty, .. } = t {
            t = ty;
        }
        t
    }

    /// Whether writes through a value of this type are forbidden because it is a
    /// `readonly` array or tuple, looking through aliases and refinements.
    pub fn is_readonly_array(&self) -> bool {
        matches!(self.peel_preserving_readonly(), Type::Readonly(_))
    }

    /// The element type of a rest parameter's array: `T` for `T[]` or
    /// `readonly T[]`. Not peeled through aliases, since rest lowering matches the
    /// parameter type as written.
    pub fn rest_element(&self) -> Option<&Type> {
        match self {
            Type::Array(element) => Some(element),
            Type::Readonly(inner) => match inner.as_ref() {
                Type::Array(element) => Some(element),
                _ => None,
            },
            _ => None,
        }
    }

    /// A rest parameter's type with any `readonly` removed. Each call packs a
    /// fresh array for the rest, so whether the callee may write to it is the
    /// callee's own concern: two function types relate on their rest elements,
    /// as in tsc.
    pub fn rest_array_ignoring_readonly(&self) -> &Type {
        match self {
            Type::Readonly(inner) if matches!(inner.as_ref(), Type::Array(_)) => inner,
            _ => self,
        }
    }

    /// Whether this is a union of only arrays and tuples, which share the `$Array`
    /// representation.
    pub fn is_array_like_union(&self) -> bool {
        match self.peel() {
            Type::Union(members) => members
                .iter()
                .all(|member| matches!(member.peel(), Type::Array(_) | Type::Tuple(_))),
            _ => false,
        }
    }

    /// The element of a union of arrays and tuples read as one array: any
    /// member's element. `None` for any other type. Only reads may go through
    /// it; writing a member's element through the joined type could store
    /// another member's element type.
    pub fn array_like_union_element(&self) -> Option<Type> {
        let Type::Union(members) = self.peel() else {
            return None;
        };
        let elements = members
            .iter()
            .map(|member| match member.peel() {
                Type::Array(element) => Some((**element).clone()),
                Type::Tuple(positions) => Some(Type::union(positions.clone())),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Type::union(elements))
    }

    /// The arrays and tuples of a union of strings with arrays or tuples, and
    /// nothing else, as their own union. Such a union has no shared
    /// representation, so each use tests `typeof` and takes the string's or the
    /// array's path.
    pub fn string_or_array_union_arrays(&self) -> Option<Type> {
        let Type::Union(members) = self.peel() else {
            return None;
        };
        let (strings, arrays): (Vec<Type>, Vec<Type>) = members
            .iter()
            .cloned()
            .partition(|member| member.peel().is_string_shaped());
        let all_arrays = arrays
            .iter()
            .all(|member| matches!(member.peel(), Type::Array(_) | Type::Tuple(_)));
        (!strings.is_empty() && !arrays.is_empty() && all_arrays).then(|| Type::union(arrays))
    }

    /// The array a union of arrays and tuples reads as: see
    /// [`Self::array_like_union_element`].
    pub fn array_like_union_view(&self) -> Option<Type> {
        self.array_like_union_element()
            .map(|element| Type::Array(Box::new(element)))
    }

    /// Whether this type is `void`, through any depth of alias.
    ///
    /// `void` is the one type with no value slot at all, so a gate that tests
    /// for it decides between "one Wasm result" and "none". Every such gate —
    /// in the typechecker *and* in codegen — must answer the same way for
    /// `type V = void` as for `void`: a bare `matches!(ty, Type::Void)` hands an
    /// aliased `void` a value slot that
    /// [`SymbolTable::value_type`](crate::codegen::symbol_table::SymbolTable::value_type)
    /// then refuses to lower. A typecheck gate must not start peeling ahead of
    /// its codegen counterpart, which turns a clean diagnostic into a panic.
    pub fn is_void(&self) -> bool {
        matches!(self.peel(), Type::Void)
    }

    /// Whether a function taking `actual` parameters can stand where one taking
    /// `expected` is called. As in TypeScript, it may declare fewer and ignore
    /// the rest of the arguments; a closure adapter drops them at runtime. Rest
    /// functions keep an exact arity, since their packed array has a slot of its
    /// own.
    pub fn function_arity_fits(actual: usize, expected: usize, has_rest: bool) -> bool {
        actual == expected || (!has_rest && actual < expected)
    }

    /// Whether a value of this type would need a `void` slot at runtime:
    /// `void` itself, or a union that lists it.
    ///
    /// Deliberately one level deep — it does **not** descend into a function's
    /// return type, where `void` is legitimate (`() => void`). Every gate that
    /// refuses `void` in a value or comparison position wants this, not the
    /// bare [`is_void`](Self::is_void). `cond ? f() : 1` and `a ?? f()` are
    /// plain `void`, so no expression builds the union form; a gate that asks
    /// this still refuses one that did, rather than letting it reach codegen.
    pub fn carries_void(&self) -> bool {
        match self.peel() {
            Type::Void => true,
            Type::Union(members) => members.iter().any(|m| matches!(m.peel(), Type::Void)),
            _ => false,
        }
    }

    /// Primitive operations shared by enums and homogeneous literal unions,
    /// without widening their type identity or changing assignability.
    pub fn primitive_behavior(&self) -> &Type {
        match self.peel() {
            Type::NumberEnum { .. } => &Type::Number,
            Type::StringEnum { .. } => &Type::String,
            Type::BooleanLiteral(_) => &Type::Boolean,
            Type::Union(members)
                if !members.is_empty()
                    && members.iter().all(|member| {
                        matches!(
                            member.primitive_behavior(),
                            Type::Number | Type::NumberLiteral(_)
                        )
                    }) =>
            {
                &Type::Number
            }
            Type::Union(members)
                if !members.is_empty() && members.iter().all(Type::is_string_shaped) =>
            {
                &Type::String
            }
            ty => ty,
        }
    }

    /// This type with literal types replaced by the primitive they are a literal of.
    ///
    /// A literal type is only sound where the value cannot change, so inference keeps
    /// it at a `const` binding and widens here at every position that is mutable or
    /// whose type is inferred from its contents — a `let` binding, an array element,
    /// an object-literal property, a generic argument. `const a = 1` is `1`, but
    /// `let b = a` is `number`, matching TypeScript.
    ///
    /// Enums are left alone: `NumberEnum`/`StringEnum` are nominal types, not literals,
    /// and widening them would discard the identity their members are checked against.
    /// Use [`primitive_behavior`](Self::primitive_behavior) for that.
    pub fn widen_literal(&self) -> Type {
        match self {
            Type::NumberLiteral(_) => Type::Number,
            Type::StringLiteral(_) => Type::String,
            Type::BooleanLiteral(_) => Type::Boolean,
            // A union widens memberwise, which also collapses it when the members
            // share a base: `1 | 2` is `number`, not `number | number`, because
            // `Type::union` deduplicates.
            Type::Union(members) => Type::union(members.iter().map(Type::widen_literal).collect()),
            _ => self.clone(),
        }
    }

    /// Whether every part of this type is a string — a `string`, a
    /// string-literal type, or a union of those.
    ///
    /// The typechecker, the import collector, and codegen must agree on this
    /// exactly: a shape accepted by one and not the others emits a `$string`
    /// into an `f64` slot, which fails Wasm validation rather than type
    /// checking. Contrast [`contains_string`](Self::contains_string), which
    /// asks the `any` question.
    pub fn is_string_shaped(&self) -> bool {
        match self.peel() {
            Type::String | Type::StringLiteral(_) | Type::StringEnum { .. } => true,
            Type::Union(members) => members.iter().all(Type::is_string_shaped),
            _ => false,
        }
    }

    /// Whether *some* part of this type is a string. Diagnostics-only: it is
    /// what decides whether a rejected operand still deserves the "convert
    /// first" help, so `string | null` gets it.
    pub fn contains_string(&self) -> bool {
        match self.peel() {
            Type::String | Type::StringLiteral(_) | Type::StringEnum { .. } => true,
            Type::Union(members) => members.iter().any(Type::contains_string),
            _ => false,
        }
    }

    /// Normalizes to a flat member list that is unique *by peeled type*.
    ///
    /// Both halves peel because a nominal key is unsound here: an alias whose
    /// body is a union is a nested union in disguise, and every downstream
    /// per-member probe (`value_type`'s nullability test, narrowing's member
    /// walk) reads it as one opaque member. Deduping nominally leaves
    /// `N | number` as a two-member union that no operator is defined on.
    ///
    /// The alias label a union displays therefore survives only where it is
    /// still a single member: an alias of a union is replaced by its members,
    /// and among duplicates the alias-labelled spelling wins regardless of
    /// source order, so `N | number` and `number | N` both keep `N`.
    ///
    /// `peel` stops at a *recursive* alias (it is a name, not a body), so a
    /// recursive alias never dedups against its own expansion. Both spellings
    /// lower the same way, so this costs a redundant member, not correctness.
    pub fn union(members: Vec<Type>) -> Type {
        if members.iter().any(|m| matches!(m.peel(), Type::Error)) {
            return Type::Error;
        }
        // `unknown | T` → `unknown`; peels aliases so `type Dyn = unknown` still collapses.
        if members.iter().any(|m| matches!(m.peel(), Type::Unknown)) {
            return Type::Unknown;
        }
        // `T | never` → `T`. After Error/Unknown checks so cascading-silence wins.
        let members: Vec<Type> = members
            .into_iter()
            .filter(|m| !matches!(m.peel(), Type::Never))
            .collect();
        let mut flat: Vec<Type> = Vec::new();
        for m in members {
            match m.without_aliases() {
                Type::Union(inner) => flat.extend(inner.iter().cloned()),
                _ => flat.push(m),
            }
        }
        // Sorting by the peeled key groups alias-labelled duplicates next to
        // their body; aliases rank 0 so they sort first within the group, and
        // `dedup_by` keeps the earlier element — the name the user wrote.
        let alias_rank = |t: &Type| u8::from(!matches!(t, Type::Alias { .. }));
        flat.sort_by(|a, b| {
            a.without_aliases()
                .cmp(b.without_aliases())
                .then_with(|| alias_rank(a).cmp(&alias_rank(b)))
        });
        flat.dedup_by(|a, b| a.without_aliases() == b.without_aliases());
        fold_boolean_literals(&mut flat);
        if flat.len() > 1 {
            return Type::Union(flat);
        }
        match flat.into_iter().next() {
            Some(member) => member,
            None => Type::Never,
        }
    }

    /// For method dispatch. Erased generics route to `Object`. Aliases peel. Returns `None`
    /// for function, void, null, and error. The first tuple element is the owning package
    /// for diagnostics; the mangled name is the structural lookup key. Built-in
    /// interfaces (`Number`, `Array`, `Object`, …) live in the prelude package.
    pub fn interface_routing(&self) -> Option<(MangledName, &str, &str, Vec<Type>)> {
        let prelude = crate::mangle::PRELUDE_PACKAGE;
        match self.primitive_behavior() {
            // A literal type routes to its base's interface: `"abc".at(0)` resolves the
            // same members a `string` receiver does. The prelude `Number`/`String`
            // interfaces expose no mutating members, so routing a literal there cannot
            // invalidate it.
            Type::Number | Type::NumberLiteral(_) => Some((
                crate::mangle::prelude("Number"),
                prelude,
                "Number",
                Vec::new(),
            )),
            Type::BigInt => Some((
                crate::mangle::prelude("BigInt"),
                prelude,
                "BigInt",
                Vec::new(),
            )),
            Type::Boolean => Some((
                crate::mangle::prelude("Boolean"),
                prelude,
                "Boolean",
                Vec::new(),
            )),
            Type::String | Type::StringLiteral(_) => Some((
                crate::mangle::prelude("String"),
                prelude,
                "String",
                Vec::new(),
            )),
            Type::Uint8Array => Some((
                crate::mangle::prelude("Uint8Array"),
                prelude,
                "Uint8Array",
                Vec::new(),
            )),
            Type::Array(elem) => Some((
                crate::mangle::prelude("Array"),
                prelude,
                "Array",
                vec![(**elem).clone()],
            )),
            // Tuples are arrays at runtime; route to `Array` with the positions'
            // union as the element type so reads (`length`, `map`, `at`, …) flow
            // through. Mutating methods are rejected separately at the call site.
            Type::Tuple(elements) => Some((
                crate::mangle::prelude("Array"),
                prelude,
                "Array",
                vec![Type::union(elements.clone())],
            )),
            Type::Object { .. } | Type::TypeVar(_) | Type::GenericParam { .. } => Some((
                crate::mangle::prelude("Object"),
                prelude,
                "Object",
                Vec::new(),
            )),
            Type::InterfaceRef {
                mangled,
                package,
                name,
                args,
                ..
            }
            | Type::ClassRef {
                mangled,
                package,
                name,
                args,
                ..
            } => Some((
                mangled.clone(),
                package.as_str(),
                name.as_str(),
                args.clone(),
            )),
            // Enum values route to Object (not Number/String) to keep the API surface minimal.
            Type::NumberEnum { .. } | Type::StringEnum { .. } => Some((
                crate::mangle::prelude("Object"),
                prelude,
                "Object",
                Vec::new(),
            )),
            // `unknown` routes to Object so vtable methods work.
            Type::Unknown => Some((
                crate::mangle::prelude("Object"),
                prelude,
                "Object",
                Vec::new(),
            )),
            _ => None,
        }
    }
}

/// `s` escaped for a double-quoted string the way `tsc` prints a string literal
/// type (`escapeString` in TypeScript's `utilities.ts`): `"G\"HI"`, `"a\nb"`.
pub(crate) fn escape_string_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{b}' => out.push_str("\\v"),
            '\u{c}' => out.push_str("\\f"),
            // `\0` before a digit would read as an octal escape.
            '\0' if chars.peek().is_some_and(char::is_ascii_digit) => out.push_str("\\x00"),
            '\0' => out.push_str("\\0"),
            '\u{0}'..='\u{1f}' | '\u{85}' | '\u{2028}' | '\u{2029}' => {
                out.push_str(&format!("\\u{:04X}", u32::from(c)));
            }
            c => out.push(c),
        }
    }
    out
}

/// `boolean` is `true | false`: a union holding both literals, or `boolean`
/// and either literal, holds exactly `boolean`. `members` is sorted and
/// deduplicated, and stays so.
fn fold_boolean_literals(members: &mut Vec<Type>) {
    let is_boolean = |m: &Type| matches!(m.without_aliases(), Type::Boolean);
    let is_literal = |m: &Type| matches!(m.without_aliases(), Type::BooleanLiteral(_));
    let literals = members.iter().filter(|m| is_literal(m)).count();
    let has_boolean = members.iter().any(is_boolean);
    if literals == 0 || (literals == 1 && !has_boolean) {
        return;
    }
    members.retain(|m| !is_literal(m));
    if !has_boolean {
        let at = members.partition_point(|m| m.without_aliases() < &Type::Boolean);
        members.insert(at, Type::Boolean);
    }
}

impl Type {
    pub fn render_checked(
        &self,
        limits: crate::rendering::RenderLimits,
    ) -> Result<crate::rendering::RenderedText, crate::rendering::RenderError> {
        crate::type_rendering::render(self, limits)
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.render_checked(crate::rendering::RenderLimits::default()) {
            Ok(rendered) => f.write_str(&rendered.text),
            Err(_) => f.write_str("[diagnostic type unavailable]"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Package, Type};

    #[test]
    fn atomic_display() {
        assert_eq!(Type::Number.to_string(), "number");
        assert_eq!(Type::String.to_string(), "string");
        assert_eq!(Type::Boolean.to_string(), "boolean");
        assert_eq!(Type::Null.to_string(), "null");
        assert_eq!(Type::Void.to_string(), "void");
        assert_eq!(Type::Error.to_string(), "<error>");
    }

    #[test]
    fn readonly_display_matches_typescript() {
        let numbers = Type::Array(Box::new(Type::Number));
        let readonly = Type::Readonly(Box::new(numbers.clone()));
        assert_eq!(readonly.to_string(), "readonly number[]");
        assert_eq!(
            Type::Array(Box::new(readonly.clone())).to_string(),
            "(readonly number[])[]"
        );
        assert_eq!(
            Type::Readonly(Box::new(Type::Array(Box::new(numbers)))).to_string(),
            "readonly number[][]"
        );
        assert_eq!(
            Type::Readonly(Box::new(Type::Tuple(vec![Type::Number, Type::String]))).to_string(),
            "readonly [number, string]"
        );
        assert!(readonly.is_readonly_array());
        assert_eq!(readonly.peel(), &Type::Array(Box::new(Type::Number)));
    }

    #[test]
    fn string_literal_display_escapes_like_typescript() {
        // Each expectation is what `tsc` 5.9 prints for the same literal type.
        let cases = [
            ("G\"HI\\", r#""G\"HI\\""#),
            ("a\nb\r\t", r#""a\nb\r\t""#),
            ("\u{8}\u{b}\u{c}", r#""\b\v\f""#),
            ("\0x", r#""\0x""#),
            ("\u{0}1", r#""\x001""#),
            ("\u{7}\u{1b}", r#""\u0007\u001B""#),
            ("\u{85}\u{2028}\u{2029}", r#""\u0085\u2028\u2029""#),
            ("\u{7f}é", "\"\u{7f}é\""),
        ];
        for (value, printed) in cases {
            assert_eq!(Type::StringLiteral(value.into()).to_string(), printed);
        }
    }

    #[test]
    fn structural_equality() {
        assert_eq!(Type::Number, Type::Number);
        assert_ne!(Type::Number, Type::String);

        let a = Type::Function {
            params: vec![Type::Number, Type::String],
            ret: Box::new(Type::Boolean),
            predicate: None,
            has_rest: false,
        };
        let b = Type::Function {
            params: vec![Type::Number, Type::String],
            ret: Box::new(Type::Boolean),
            predicate: None,
            has_rest: false,
        };
        let c = Type::Function {
            params: vec![Type::Number],
            ret: Box::new(Type::Boolean),
            predicate: None,
            has_rest: false,
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn function_display_no_params() {
        let t = Type::Function {
            params: vec![],
            ret: Box::new(Type::Number),
            predicate: None,
            has_rest: false,
        };
        assert_eq!(t.to_string(), "() => number");
    }

    #[test]
    fn function_display_one_param() {
        let t = Type::Function {
            params: vec![Type::Number],
            ret: Box::new(Type::Boolean),
            predicate: None,
            has_rest: false,
        };
        assert_eq!(t.to_string(), "(arg0: number) => boolean");
    }

    #[test]
    fn function_display_multi_param() {
        let t = Type::Function {
            params: vec![Type::Number, Type::String],
            ret: Box::new(Type::Void),
            predicate: None,
            has_rest: false,
        };
        assert_eq!(t.to_string(), "(arg0: number, arg1: string) => void");
    }

    #[test]
    fn function_display_nested() {
        let inner = Type::Function {
            params: vec![],
            ret: Box::new(Type::Number),
            predicate: None,
            has_rest: false,
        };
        let outer = Type::Function {
            params: vec![],
            ret: Box::new(inner),
            predicate: None,
            has_rest: false,
        };
        assert_eq!(outer.to_string(), "() => () => number");
    }

    #[test]
    fn clone_roundtrip() {
        let t = Type::Function {
            params: vec![Type::Number, Type::String],
            ret: Box::new(Type::Boolean),
            predicate: None,
            has_rest: false,
        };
        assert_eq!(t.clone(), t);
    }

    fn point_type() -> Type {
        let mut fields = std::collections::BTreeMap::new();
        fields.insert("x".to_string(), crate::ObjectField::required(Type::Number));
        fields.insert("y".to_string(), crate::ObjectField::required(Type::Number));
        Type::Object {
            index: None,
            fields,
        }
    }

    #[test]
    fn object_structural_equality_ignores_field_insertion_order() {
        let mut a = std::collections::BTreeMap::new();
        a.insert("y".to_string(), crate::ObjectField::required(Type::Number));
        a.insert("x".to_string(), crate::ObjectField::required(Type::Number));

        let mut b = std::collections::BTreeMap::new();
        b.insert("x".to_string(), crate::ObjectField::required(Type::Number));
        b.insert("y".to_string(), crate::ObjectField::required(Type::Number));

        assert_eq!(
            Type::Object {
                index: None,
                fields: a
            },
            Type::Object {
                index: None,
                fields: b
            }
        );
    }

    #[test]
    fn object_inequality_on_different_field_set() {
        let mut a = std::collections::BTreeMap::new();
        a.insert("x".to_string(), crate::ObjectField::required(Type::Number));
        let mut b = std::collections::BTreeMap::new();
        b.insert("y".to_string(), crate::ObjectField::required(Type::Number));
        assert_ne!(
            Type::Object {
                index: None,
                fields: a
            },
            Type::Object {
                index: None,
                fields: b
            }
        );
    }

    #[test]
    fn object_display() {
        assert_eq!(point_type().to_string(), "{ x: number; y: number }");
        let empty = Type::Object {
            index: None,
            fields: std::collections::BTreeMap::new(),
        };
        assert_eq!(empty.to_string(), "{}");
    }

    #[test]
    fn object_display_with_optional_field() {
        let mut fields = std::collections::BTreeMap::new();
        fields.insert("x".to_string(), crate::ObjectField::required(Type::Number));
        fields.insert("y".to_string(), crate::ObjectField::optional(Type::String));
        let t = Type::Object {
            index: None,
            fields,
        };
        assert_eq!(t.to_string(), "{ x: number; y?: string }");
    }

    #[test]
    fn object_inequality_required_vs_optional() {
        let mut a = std::collections::BTreeMap::new();
        a.insert("x".to_string(), crate::ObjectField::required(Type::Number));
        let mut b = std::collections::BTreeMap::new();
        b.insert("x".to_string(), crate::ObjectField::optional(Type::Number));
        assert_ne!(
            Type::Object {
                index: None,
                fields: a
            },
            Type::Object {
                index: None,
                fields: b
            }
        );
    }

    #[test]
    fn array_equality() {
        assert_eq!(
            Type::Array(Box::new(Type::Number)),
            Type::Array(Box::new(Type::Number))
        );
        assert_ne!(
            Type::Array(Box::new(Type::Number)),
            Type::Array(Box::new(Type::String))
        );
    }

    #[test]
    fn array_display() {
        assert_eq!(Type::Array(Box::new(Type::Number)).to_string(), "number[]");
        assert_eq!(
            Type::Array(Box::new(Type::Array(Box::new(Type::Number)))).to_string(),
            "number[][]"
        );
    }

    #[test]
    fn nested_object_in_array_display() {
        let arr = Type::Array(Box::new(point_type()));
        assert_eq!(arr.to_string(), "{ x: number; y: number }[]");
    }

    #[test]
    fn type_var_display_uses_source_name() {
        assert_eq!(Type::TypeVar("T".to_string()).to_string(), "T");
        assert_eq!(Type::TypeVar("Key".to_string()).to_string(), "Key");
    }

    #[test]
    fn type_var_structural_equality_by_name() {
        let t = Type::TypeVar("T".to_string());
        let t2 = Type::TypeVar("T".to_string());
        let u = Type::TypeVar("U".to_string());
        assert_eq!(t, t2);
        assert_ne!(t, u);
        assert_ne!(t, Type::Number);
    }

    #[test]
    fn type_var_inside_array_and_function_displays_through() {
        let arr = Type::Array(Box::new(Type::TypeVar("T".to_string())));
        assert_eq!(arr.to_string(), "T[]");
        let func = Type::Function {
            params: vec![Type::TypeVar("T".to_string())],
            ret: Box::new(Type::TypeVar("U".to_string())),
            predicate: None,
            has_rest: false,
        };
        assert_eq!(func.to_string(), "(arg0: T) => U");
    }

    #[test]
    fn generic_param_display_uses_name_not_id() {
        let gp = Type::GenericParam {
            id: 42,
            name: "T".to_string(),
        };
        assert_eq!(gp.to_string(), "T");
    }

    #[test]
    fn generic_param_equality_by_id() {
        let a = Type::GenericParam {
            id: 5,
            name: "T".to_string(),
        };
        let b = Type::GenericParam {
            id: 5,
            name: "T".to_string(),
        };
        assert_eq!(a, b);

        let c = Type::GenericParam {
            id: 6,
            name: "T".to_string(),
        };
        assert_ne!(a, c);
    }

    #[test]
    fn union_canonical_dedup() {
        let t = Type::union(vec![Type::Number, Type::Null, Type::Number]);
        let Type::Union(members) = t else {
            panic!("expected Union, got {t:?}");
        };
        assert_eq!(members, vec![Type::Number, Type::Null]);
    }

    #[test]
    fn union_canonical_order_independent() {
        let a = Type::union(vec![Type::Number, Type::Null]);
        let b = Type::union(vec![Type::Null, Type::Number]);
        assert_eq!(a, b);
    }

    #[test]
    fn union_collapses_to_single_member() {
        let t = Type::union(vec![Type::Number, Type::Number]);
        assert_eq!(t, Type::Number);
    }

    #[test]
    fn union_flattens_nested() {
        let inner = Type::union(vec![Type::String, Type::Null]);
        let outer = Type::union(vec![Type::Number, inner]);
        let expected = Type::union(vec![Type::Number, Type::String, Type::Null]);
        assert_eq!(outer, expected);
    }

    #[test]
    fn union_error_member_collapses_to_error() {
        let t = Type::union(vec![Type::Number, Type::Error]);
        assert_eq!(t, Type::Error);
    }

    #[test]
    fn union_display_basic() {
        let t = Type::union(vec![Type::Number, Type::String]);
        assert_eq!(t.to_string(), "number | string");
    }

    #[test]
    fn union_display_with_null() {
        let t = Type::union(vec![Type::Number, Type::Null]);
        assert_eq!(t.to_string(), "number | null");
        let t2 = Type::union(vec![Type::Null, Type::Number]);
        assert_eq!(t2.to_string(), "number | null");
    }

    #[test]
    fn union_display_wraps_function_member() {
        let fn_ty = Type::Function {
            params: vec![],
            ret: Box::new(Type::String),
            predicate: None,
            has_rest: false,
        };
        let t = Type::union(vec![fn_ty, Type::Boolean]);
        assert_eq!(t.to_string(), "boolean | (() => string)");
    }

    use super::LiteralF64;

    #[test]
    fn string_literal_display_is_quoted() {
        let t = Type::StringLiteral("north".to_string());
        assert_eq!(t.to_string(), "\"north\"");
    }

    #[test]
    fn number_literal_display_is_unquoted_with_whole_number_shape() {
        let whole = Type::NumberLiteral(LiteralF64(42.0));
        assert_eq!(whole.to_string(), "42");
        let frac = Type::NumberLiteral(LiteralF64(4.5));
        assert_eq!(frac.to_string(), "4.5");
    }

    #[test]
    fn string_literal_equality_by_value() {
        let a = Type::StringLiteral("hi".to_string());
        let b = Type::StringLiteral("hi".to_string());
        let c = Type::StringLiteral("bye".to_string());
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn number_literal_equality_by_value() {
        let a = Type::NumberLiteral(LiteralF64(42.0));
        let b = Type::NumberLiteral(LiteralF64(42.0));
        let c = Type::NumberLiteral(LiteralF64(43.0));
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn union_sorts_literals_next_to_base_types() {
        // NumberLiteral sorts after Number, StringLiteral after String per variant order.
        let t = Type::union(vec![
            Type::String,
            Type::StringLiteral("hi".to_string()),
            Type::Number,
            Type::NumberLiteral(LiteralF64(42.0)),
        ]);
        assert_eq!(t.to_string(), "number | 42 | string | \"hi\"");
    }

    #[test]
    fn widen_literal_replaces_a_literal_with_its_base() {
        assert_eq!(
            Type::NumberLiteral(LiteralF64(1.0)).widen_literal(),
            Type::Number
        );
        assert_eq!(
            Type::StringLiteral("hi".to_string()).widen_literal(),
            Type::String
        );
    }

    /// A union widens memberwise, and `Type::union` then deduplicates — so a union of
    /// literals over one base collapses to that base rather than repeating it.
    #[test]
    fn widen_literal_collapses_a_union_of_literals() {
        let t = Type::union(vec![
            Type::NumberLiteral(LiteralF64(1.0)),
            Type::NumberLiteral(LiteralF64(2.0)),
        ]);
        assert_eq!(t.widen_literal(), Type::Number);
    }

    /// Enums are nominal, not literal: widening one would discard the identity its
    /// members are checked against. `primitive_behavior` is the enum-aware accessor.
    #[test]
    fn widen_literal_leaves_everything_else_alone() {
        assert_eq!(Type::Number.widen_literal(), Type::Number);
        assert_eq!(Type::Boolean.widen_literal(), Type::Boolean);
        let arr = Type::Array(Box::new(Type::NumberLiteral(LiteralF64(1.0))));
        assert_eq!(arr.widen_literal(), arr);
    }

    #[test]
    fn union_of_two_string_literals_sorts_by_value() {
        let t = Type::union(vec![
            Type::StringLiteral("south".to_string()),
            Type::StringLiteral("north".to_string()),
        ]);
        assert_eq!(t.to_string(), "\"north\" | \"south\"");
    }

    #[test]
    fn literal_f64_equality_distinguishes_negative_zero_at_bit_level() {
        // Bit-pattern equality distinguishes -0.0 and +0.0; parser canonicalizes -0.0→0.0 so
        // this case won't appear in practice.
        let pos = LiteralF64(0.0);
        let neg = LiteralF64(-0.0);
        assert_ne!(pos, neg);
    }

    #[test]
    fn type_var_and_generic_param_are_distinct_types() {
        // TypeVar is signature form; GenericParam is body form.
        // Even with the same name they're different variants.
        let tv = Type::TypeVar("T".to_string());
        let gp = Type::GenericParam {
            id: 0,
            name: "T".to_string(),
        };
        assert_ne!(tv, gp);
    }

    #[test]
    fn alias_display_renders_name_not_body() {
        let ty = user_alias("Circle", point_type());
        assert_eq!(ty.to_string(), "Circle");
    }

    #[test]
    fn peel_walks_through_alias_to_underlying() {
        let inner = Type::Number;
        let aliased = user_alias("ID", inner.clone());
        assert_eq!(aliased.peel(), &inner);
    }

    #[test]
    fn peel_is_idempotent_through_nested_aliases() {
        let nested = user_alias("A", user_alias("B", Type::Number));
        assert_eq!(nested.peel(), &Type::Number);
        assert_eq!(nested.peel().peel(), &Type::Number);
    }

    #[test]
    fn alias_is_not_structurally_equal_to_underlying() {
        let aliased = user_alias("ID", Type::Number);
        assert_ne!(aliased, Type::Number);
        assert_eq!(aliased.peel(), &Type::Number);
    }

    #[test]
    fn alias_routes_to_underlying_interface() {
        let aliased = user_alias("ID", Type::Number);
        let (_mangled, _pkg, iface, _args) = aliased.interface_routing().expect("number routes");
        assert_eq!(iface, "Number");
    }

    #[test]
    fn union_of_error_alias_collapses_to_error() {
        let alias_err = user_alias("Bad", Type::Error);
        let t = Type::union(vec![Type::Number, alias_err]);
        assert_eq!(t, Type::Error);
    }

    #[test]
    fn union_preserves_distinct_alias_members_for_display() {
        let circle = user_alias("Circle", point_type());
        let rect = user_alias("Rectangle", Type::Number);
        let t = Type::union(vec![circle, rect]);
        // Ordered by the peeled bodies, which puts `Number` before the object
        // shape — not by the order the members were written.
        assert_eq!(t.to_string(), "Rectangle | Circle");
    }

    #[test]
    fn union_dedups_an_alias_against_its_own_body_keeping_the_label() {
        let n = user_alias("N", Type::Number);
        assert_eq!(Type::union(vec![n.clone(), Type::Number]), n);
        assert_eq!(Type::union(vec![Type::Number, n.clone()]), n);
    }

    #[test]
    fn union_dedups_aliases_that_share_one_body() {
        let circle = user_alias("Circle", point_type());
        let rect = user_alias("Rectangle", point_type());
        assert_eq!(Type::union(vec![circle.clone(), rect]), circle);
    }

    #[test]
    fn union_flattens_an_alias_whose_body_is_a_union() {
        let nullable = user_alias("MN", Type::union(vec![Type::Number, Type::Null]));
        assert_eq!(
            Type::union(vec![nullable, Type::Number]),
            Type::union(vec![Type::Number, Type::Null])
        );
    }

    #[test]
    fn generic_alias_display_includes_args() {
        let ty = user_alias_args("Box", vec![Type::Number], Type::Number);
        assert_eq!(ty.to_string(), "Box<number>");
    }

    fn iface(package: &str, name: &str) -> Type {
        Type::interface_ref(
            Package(package.to_string()),
            name,
            crate::mangle::package_symbol(package, name),
            Vec::new(),
        )
    }

    fn user_alias(name: &str, ty: Type) -> Type {
        user_alias_args(name, Vec::new(), ty)
    }

    fn user_alias_args(name: &str, args: Vec<Type>, ty: Type) -> Type {
        Type::alias_ty(
            Package::user(),
            name,
            crate::mangle::package_symbol(crate::mangle::USER_PACKAGE, name),
            args,
            Box::new(ty),
        )
    }

    #[test]
    fn mangled_name_keys_type_identity() {
        // Identity is the mangled name. Same `(package, name)` ⇒ same mangled ⇒ equal;
        // a different owning package ⇒ different mangled ⇒ distinct (the multi-file
        // property: two same-named interfaces in different packages/modules are not
        // conflated).
        assert_eq!(iface("main", "Response"), iface("main", "Response"));
        let http = iface("submilli:http", "Response");
        let main = iface("main", "Response");
        assert_ne!(
            http, main,
            "different package ⇒ different mangled ⇒ distinct"
        );
        assert_ne!(
            http.cmp(&main),
            std::cmp::Ordering::Equal,
            "Ord must distinguish distinct mangled names"
        );
        assert_ne!(
            iface("main", "Response"),
            iface("main", "Other"),
            "different names stay distinct"
        );
    }

    #[test]
    fn same_name_different_module_not_equal_nor_assignable() {
        // The core multi-file guarantee: a sibling module's `Logger`
        // (`mod:main#util#Logger`) is a distinct type from the root's `Logger`
        // (`main#Logger`), even though the bare name matches.
        let root = iface("main", "Logger");
        let sibling = Type::InterfaceRef {
            mangled: crate::mangle::package_module_symbol("main", "util", "Logger"),
            package: Package::user(),
            name: "Logger".to_string(),
            args: Vec::new(),
        };
        assert_ne!(root, sibling);
        // Same mangled ⇒ equal.
        let root2 = iface("main", "Logger");
        assert_eq!(root, root2);
    }

    #[test]
    fn union_dedups_interface_refs_by_mangled() {
        // Same mangled collapses to one member; different mangled stays a 2-member union.
        let same = Type::union(vec![iface("main", "Response"), iface("main", "Response")]);
        assert!(
            !matches!(same, Type::Union(_)),
            "identical refs collapse to one member, got {same:?}"
        );
        let distinct = Type::union(vec![
            iface("submilli:http", "Response"),
            iface("main", "Response"),
        ]);
        assert!(
            matches!(distinct, Type::Union(ref ms) if ms.len() == 2),
            "distinct-package refs stay a 2-member union, got {distinct:?}"
        );
    }

    #[test]
    fn generic_alias_display_multiple_args() {
        let ty = user_alias_args("Pair", vec![Type::String, Type::Number], Type::Number);
        assert_eq!(ty.to_string(), "Pair<string, number>");
    }
}
