use std::collections::BTreeMap;

use crate::arena::{self, ArenaError, ArenaKind};
use crate::typechecker::infer::narrowing::{CastInfo, ReferencePath};
use crate::{BinOp, BindingKind, ExprId, Ident, MangledName, Span, StmtId, Type, UnOp};

#[derive(Clone, Debug, PartialEq)]
pub struct TypedExpr {
    pub kind: TypedExprKind,
    pub span: Span,
    pub ty: Type,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TypedExprKind {
    Number(f64),
    /// bigint literal — no `n` suffix, no sign; negatives are `Unary { Neg, … }`.
    BigInt(String),
    String(String),
    Boolean(bool),
    Null,
    /// `this` inside a class method or constructor body. A leaf; `TypedExpr.ty`
    /// carries the enclosing class's `Type::ClassRef`. Codegen lowers it to
    /// `local.get <this-slot>` (the receiver param for methods, the allocated
    /// instance local for constructors).
    This,
    Regex {
        source: String,
        flags: String,
    },
    LocalRef {
        ident: Ident,
        boxed: bool,
    },
    /// Narrowed shadow of a `LocalRef`/`GlobalRef`. `binding` holds the synthetic
    /// `#narrow_<N>` ident so tooling can recover `path` without string-matching the
    /// `#` prefix. No `boxed` field — shadows are region-scoped and never mutated.
    LocalNarrowRef {
        binding: Ident,
        path: crate::typechecker::infer::narrowing::ReferencePath,
    },
    GlobalRef {
        mangled: MangledName,
        name: Ident,
    },
    /// Distinct from `GlobalRef` — codegen lowers to a closure adapter, not `global.get`.
    FunctionRef {
        mangled: MangledName,
        name: Ident,
    },
    /// Evaluate `effect`, discard its value, then yield `result`.
    ///
    /// No surface syntax lowers to this — the comma operator is out of scope.
    /// It exists so a fold that decides an expression's *value* statically can
    /// still keep the computation that produced the operand: `typeof f() ===
    /// "number"` has a constant answer when `f`'s return type decides the tag,
    /// but JS evaluates the operand either way.
    EffectThen {
        effect: ExprId,
        result: ExprId,
    },
    /// Run `stmts` in order, then yield `result`. An assignment used as a value
    /// lowers to this: its assignment statement, then a read of what it assigned.
    /// `stmts` declares no binding that outlives the expression.
    Sequence {
        stmts: Vec<StmtId>,
        result: ExprId,
    },
    Binary {
        op: BinOp,
        lhs: ExprId,
        rhs: ExprId,
    },
    Unary {
        op: UnOp,
        operand: ExprId,
    },
    /// Static dispatch. Closure-through-value invocation goes through `CallClosure` instead.
    Call {
        mangled: MangledName,
        args: Vec<ExprId>,
        /// Type guard predicate from callee's signature; codegen ignores it.
        type_predicate: Option<Box<crate::TypePredicate>>,
    },
    /// `call_ref` dispatch; distinct from `Call` (direct `call` instruction).
    CallClosure {
        callee: ExprId,
        args: Vec<ExprId>,
    },
    /// `@mcp/<server>.<tool>(args)` — dispatched through the single
    /// `submilli:mcp.call` host fn rather than a per-tool import. Carries the
    /// server and tool names directly, so codegen needs no mangled-name parsing.
    /// The result type (on the containing `TypedExpr`) drives how the returned JSON
    /// text is materialized (a `string`, or a typed value via the `JSON.parse`
    /// validator).
    McpCall {
        server: String,
        tool: String,
        args: Vec<ExprId>,
    },
    /// Callee signature has bare `Type::TypeVar` slots. Per-position `is_generic` flags
    /// and `return_cast` drive box/cast at the call boundary. Always statically dispatched
    /// (no generic closures).
    GenericCall {
        mangled: MangledName,
        type_args: Vec<Type>,
        args: Vec<GenericArgument>,
        /// `Some(T)` when the unsubstituted return is a bare `TypeVar`; codegen emits a
        /// cast to materialize the call-site type. `None` when return is already concrete.
        return_cast: Option<crate::Type>,
        type_predicate: Option<Box<crate::TypePredicate>>,
    },
    /// Intrinsic recognised by name in the inferer; bypasses the normal `Call` path.
    IntrinsicCall {
        kind: Intrinsic,
        args: Vec<ExprId>,
    },
    /// Interface method dispatch. Codegen composes `<iface>#<name>` to find the
    /// direct-dispatch thunk, falling back to vtable dispatch.
    MethodCall {
        receiver: ExprId,
        iface: MangledName,
        name: Ident,
        args: Vec<ExprId>,
        type_predicate: Option<Box<crate::TypePredicate>>,
    },
    /// `super(...)` in a subclass constructor: a direct call of the parent's
    /// constructor *init* fn on the current `this` (self-first ABI). Codegen
    /// pushes `this` then the args. Type is `Void`.
    SuperCtorCall {
        parent: MangledName,
        args: Vec<ExprId>,
    },
    /// `super.method(...)`: a direct call of the parent body that *declares* the
    /// method (`owner`), skipping vtable dispatch so it doesn't re-resolve to the
    /// override. Codegen pushes `this` then the args.
    SuperMethodCall {
        owner: MangledName,
        name: Ident,
        args: Vec<ExprId>,
    },
    /// `MethodSig` has a bare `TypeVar` in args or return. HOF methods (`map`, `filter`, …)
    /// with `TypeVar` only inside composites stay as plain `MethodCall`. Variadic trailing
    /// args are pre-packed by the typechecker, so `args.len()` always matches `params.len()` 1:1.
    GenericMethodCall {
        receiver: ExprId,
        iface: MangledName,
        name: Ident,
        args: Vec<GenericArgument>,
        return_cast: Option<crate::Type>,
        /// Guard predicate after type-arg substitution; codegen ignores it.
        type_predicate: Option<Box<crate::TypePredicate>>,
    },
    /// `fields` records the static shape in canonical order. `members` retains
    /// source evaluation order. In a spread literal, ordinary properties become
    /// singleton object sources so each source can be copied immediately.
    ObjectLiteral {
        /// Every expression the literal evaluates, in source order, as JavaScript
        /// evaluates them. A value a later member overwrites is still here: it is
        /// evaluated for its effects and then discarded. Each field's source
        /// refers to one of these.
        members: Vec<TypedObjectMember>,
        fields: Vec<TypedObjectFieldOrigin>,
    },
    /// `element_ty` lets codegen pick the wrapper struct's element type without re-running inference.
    ArrayLiteral {
        elements: Vec<TypedArrayElement>,
        element_ty: Type,
    },
    /// Same source syntax as `ArrayLiteral`; emitted when the expected type is `Type::Tuple`.
    /// `element_types` drives per-slot boxing in codegen.
    TupleLiteral {
        elements: Vec<ExprId>,
        element_types: Vec<Type>,
    },
    FieldAccess {
        receiver: ExprId,
        name: Ident,
    },
    /// Distinct from `FieldAccess` — codegen composes `<iface>#<name>` rather than a struct field offset.
    InterfacePropertyAccess {
        receiver: ExprId,
        iface: MangledName,
        name: Ident,
    },
    /// Enum name is a compile-time namespace with no runtime representation; `value` is
    /// stored inline so codegen emits `i32.const N`.
    NumberEnumMember {
        enum_mangled: MangledName,
        variant: Ident,
        value: f64,
    },
    /// Codegen emits a string-pool reference rather than `i32.const`.
    StringEnumMember {
        enum_mangled: MangledName,
        variant: Ident,
        value: String,
    },
    IndexAccess {
        receiver: ExprId,
        index: ExprId,
    },
    /// `captured` is empty until the capture pass runs; codegen reads entries in order as env struct slots.
    Closure {
        runtime_generics: Vec<String>,
        params: Vec<TypedParam>,
        return_type: Type,
        body: ClosureBody,
        captured: Vec<CapturedVar>,
    },
    /// `typeof x === "<tag>"` check. Distinct from `Is` so primitive type-test codegen
    /// stays free of vtable/shape-disjunction logic (`Object` and `Function` need both).
    TypeofTag {
        value: ExprId,
        tag: TypeofTagKind,
    },
    /// Wraps the dependent side of `&&`/`||`/ternary. See `docs/narrowing.md` primitive #6.
    Narrowed {
        path: ReferencePath,
        source: ExprId,
        binding: Ident,
        cast_info: CastInfo,
        inner: ExprId,
    },
    /// Narrowing on `cond` flows into `then_` (true env) and `else_` (false env).
    Ternary {
        cond: ExprId,
        then_: ExprId,
        else_: ExprId,
    },
    /// `a ?? b`. Result type is `union(strip_null(lhs.ty), rhs.ty)`.
    NullishCoalesce {
        lhs: ExprId,
        rhs: ExprId,
    },
    /// Each part carries `result_ty` so codegen steps through without re-running inference.
    /// Containing `TypedExpr.ty` is `union(tail_result_ty, Null)`.
    OptionalChain {
        base: ExprId,
        parts: Vec<TypedChainPart>,
    },
    /// Statement-position uses are desugared into `AssignLocal`/`AssignGlobal`/etc. before
    /// codegen; only expression-position uses reach here.
    PostfixUnary {
        op: crate::PostfixOp,
        target: PostfixTarget,
    },
    /// `value!` — runtime-checked non-null assertion. Throws `Error` on `null`.
    NonNullAssert {
        value: ExprId,
    },
    /// `value as target_ty`. `check` carries the structural shape to validate at runtime
    /// (the target with interfaces reduced to object shapes) when the source isn't a static
    /// subtype of the target; `None` for a statically-proven upcast (repr-only narrow, no
    /// runtime test).
    Cast {
        value: ExprId,
        target_ty: Type,
        check: Option<Box<Type>>,
    },
    /// `x instanceof Foo` — lowers to `ref.test (ref $Foo)`. `class` is the resolved
    /// `Type::ClassRef`; the result type is `boolean`.
    InstanceOf {
        value: ExprId,
        class: Type,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum PostfixTarget {
    /// `boxed` is set by Capture when an inner closure references this binding.
    Local {
        ident: Ident,
        boxed: bool,
        target_ty: Type,
    },
    /// Only `let` globals; `const` global targets are rejected by the inferer.
    Global {
        name: Ident,
        mangled: MangledName,
        target_ty: Type,
    },
    /// Receiver is cached in an anonymous local to avoid double-evaluating side effects.
    Field {
        receiver: ExprId,
        name: Ident,
        target_ty: Type,
    },
    /// Both `receiver` and `index` are cached in anonymous locals to avoid double-evaluation.
    Index {
        receiver: ExprId,
        index: ExprId,
        elem_ty: Type,
    },
}

/// `optional` drives short-circuit codegen; `result_ty` is the type after this part, without `| null`.
#[derive(Clone, Debug, PartialEq)]
pub enum TypedChainPart {
    Field {
        name: Ident,
        optional: bool,
        result_ty: Type,
        span: Span,
    },
    InterfaceProperty {
        iface: MangledName,
        name: Ident,
        optional: bool,
        result_ty: Type,
        span: Span,
    },
    Index {
        idx: ExprId,
        optional: bool,
        result_ty: Type,
        span: Span,
    },
    Call {
        args: Vec<ExprId>,
        optional: bool,
        result_ty: Type,
        span: Span,
    },
    MethodCall {
        iface: MangledName,
        name: Ident,
        args: Vec<ExprId>,
        optional: bool,
        result_ty: Type,
        span: Span,
    },
    /// `!` applied to the step before it. A runtime-checked narrowing that throws
    /// `TypeError` on null, matching the non-chain `NonNullAssert` — not TS's
    /// unchecked assertion.
    NonNull { result_ty: Type, span: Span },
}

impl TypedChainPart {
    /// The type this step yields, which is the next step's receiver.
    pub fn result_ty(&self) -> &Type {
        match self {
            TypedChainPart::Field { result_ty, .. }
            | TypedChainPart::InterfaceProperty { result_ty, .. }
            | TypedChainPart::Index { result_ty, .. }
            | TypedChainPart::Call { result_ty, .. }
            | TypedChainPart::MethodCall { result_ty, .. }
            | TypedChainPart::NonNull { result_ty, .. } => result_ty,
        }
    }

    /// Replaces the type this step yields. Used when a chain step's path turns
    /// out to be narrowed: the step reads at the narrowed type, not the declared
    /// one, and the next step's receiver follows.
    pub fn set_result_ty(&mut self, ty: Type) {
        match self {
            TypedChainPart::Field { result_ty, .. }
            | TypedChainPart::InterfaceProperty { result_ty, .. }
            | TypedChainPart::Index { result_ty, .. }
            | TypedChainPart::Call { result_ty, .. }
            | TypedChainPart::MethodCall { result_ty, .. }
            | TypedChainPart::NonNull { result_ty, .. } => *result_ty = ty,
        }
    }

    /// Whether this step short-circuits on a null receiver (`?.`).
    pub fn is_optional(&self) -> bool {
        match self {
            TypedChainPart::Field { optional, .. }
            | TypedChainPart::InterfaceProperty { optional, .. }
            | TypedChainPart::Index { optional, .. }
            | TypedChainPart::Call { optional, .. }
            | TypedChainPart::MethodCall { optional, .. } => *optional,
            TypedChainPart::NonNull { .. } => false,
        }
    }
}

/// `is_generic` = the callee's unsubstituted slot was a bare `TypeVar`; codegen emits `cast::emit_box` when set.
#[derive(Clone, Debug, PartialEq)]
pub struct GenericArgument {
    pub expr: ExprId,
    pub is_generic: bool,
}

/// `boxed: true` — env stores `(ref $box T)`, mutations go through box (let/param captures).
/// `boxed: false` — env stores the value directly (const captures, copied at construction).
#[derive(Clone, Debug, PartialEq)]
pub struct CapturedVar {
    pub name: Ident,
    pub ty: Type,
    pub boxed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ClosureBody {
    Expr(ExprId),
    Block(StmtId),
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypedObjectLiteralField {
    pub name: Ident,
    pub value: ExprId,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypedObjectFieldOrigin {
    pub name: Ident,
    pub source: TypedObjectFieldSource,
    /// Carried so the shape collector and codegen build the object's struct
    /// with the right optional flags — an absent optional must read back null
    /// and be omitted by `JSON.stringify`, not materialized as a `null` slot.
    pub optional: bool,
    /// The field's declared type, not the source value's type. They diverge for
    /// a null-filled optional field (value is `Type::Null`, declared type is the
    /// real type) and for a literal widened to its hint. The shape collector and
    /// codegen must build the object's struct — and its `TypeInfo` — from this so
    /// `JSON.stringify` serializes by the declared type, not the fill value.
    pub ty: Type,
}

/// The structural type an object literal of type `ty` is built with. Its fields
/// come from `fields`, which hold the null-filled optional fields and the
/// declared field types a literal's own type can leave out or narrow. A type
/// with an index signature (from a spread) is already the layout.
pub fn object_literal_layout(ty: &Type, fields: &[TypedObjectFieldOrigin]) -> Type {
    if let Type::Object { index: Some(_), .. } = ty {
        return ty.clone();
    }
    let fields = fields
        .iter()
        .map(|field| {
            (
                field.name.name.clone(),
                crate::ObjectField {
                    ty: field.ty.clone(),
                    optional: field.optional,
                    readonly: false,
                    method: false,
                },
            )
        })
        .collect();
    Type::Object {
        index: None,
        fields,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TypedObjectFieldSource {
    Literal(ExprId),
    Absent(ExprId),
    /// `source_index` counts the literal's `Spread` members; `source_ty` is the
    /// spread's `Type::Object` so codegen can index by BTreeMap order.
    Spread {
        source_index: usize,
        field_name: String,
        source_ty: Type,
        /// Set when the source's field is optional and an earlier member wrote
        /// the same field: an absent field leaves that earlier value in place,
        /// as in JavaScript, where `{ a: 1, ...{} }` keeps `a: 1`.
        fallback: Option<Box<TypedObjectFieldSource>>,
    },
}

impl TypedObjectFieldSource {
    pub fn literal_expr_id(&self) -> Option<ExprId> {
        match self {
            TypedObjectFieldSource::Literal(id) | TypedObjectFieldSource::Absent(id) => Some(*id),
            TypedObjectFieldSource::Spread { .. } => None,
        }
    }
}

/// One member of an object literal, in source order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TypedObjectMember {
    Computed {
        key: ExprId,
        value: ExprId,
    },
    /// A field's value.
    Value(ExprId),
    /// A spread's source object. `by_name` is set when the source has no one
    /// layout — its type is a union of object types, or it is a conditional
    /// whose branches differ — so each field is found by name at run time, and
    /// one the object lacks reads as absent.
    Spread {
        source: ExprId,
        by_name: bool,
    },
}

impl TypedObjectMember {
    pub fn expressions(self) -> impl Iterator<Item = ExprId> {
        let key = match self {
            Self::Computed { key, .. } => Some(key),
            _ => None,
        };
        key.into_iter().chain(std::iter::once(self.expr_id()))
    }

    pub fn expr_id(self) -> ExprId {
        match self {
            TypedObjectMember::Value(id)
            | TypedObjectMember::Spread { source: id, .. }
            | TypedObjectMember::Computed { value: id, .. } => id,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TypedArrayElement {
    Value(ExprId),
    Spread(ExprId),
}

impl TypedArrayElement {
    pub fn expr_id(&self) -> ExprId {
        match self {
            TypedArrayElement::Value(id) | TypedArrayElement::Spread(id) => *id,
        }
    }
}

/// `"undefined"` is unsupported — Submilli has no `undefined`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TypeofTagKind {
    Number,
    String,
    Boolean,
    /// `"object"` — includes `null` (the JS quirk) and arrays; excludes functions.
    Object,
    /// `"function"` — vtable equality check against shared `closure_vtable` global.
    Function,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Intrinsic {
    /// `assert(cond, msg)` — `unreachable` on false; will throw `Error` post-exceptions.
    Assert,
    /// `JSON.stringify(x)` — only the call form is accepted; bare `JSON` ref is rejected.
    JsonStringify,
    /// `JSON.parse(s)` — returns `unknown`; use `as T` for runtime validation.
    JsonParse,
    /// `BigInt.fromString(s)` — decimal parse via `submilli:bigint.fromString`; throws on failure.
    BigIntFromString,
}

impl Intrinsic {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "assert" => Some(Intrinsic::Assert),
            _ => None,
        }
    }

    /// `JsonStringify` uses `Type::Error` as the value slot — typechecker accepts any
    /// non-`void` arg; only diagnostic formatting reads this.
    pub fn params(self) -> Vec<crate::Param> {
        use crate::{Param, Type};
        match self {
            Intrinsic::Assert => vec![
                Param::new("condition", Type::Boolean),
                Param {
                    name: "message".to_string(),
                    ty: Type::String,
                    default: Some(crate::DefaultValue::String("assertion failed".to_string())),
                    rest: false,
                },
            ],
            Intrinsic::BigIntFromString => vec![Param::new("value", Type::String)],
            Intrinsic::JsonStringify => vec![Param::new("value", Type::Error)],
            Intrinsic::JsonParse => vec![Param::new("text", Type::String)],
        }
    }

    /// `JsonParse` returns `unknown`.
    pub fn ret(self) -> crate::Type {
        match self {
            Intrinsic::Assert => crate::Type::Void,
            Intrinsic::BigIntFromString => crate::Type::BigInt,
            Intrinsic::JsonStringify => crate::Type::String,
            Intrinsic::JsonParse => crate::Type::Unknown,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Intrinsic::Assert => "assert",
            Intrinsic::BigIntFromString => "BigInt.fromString",
            Intrinsic::JsonStringify => "JSON.stringify",
            Intrinsic::JsonParse => "JSON.parse",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypedStmt {
    pub kind: TypedStmtKind,
    pub span: Span,
}

/// Resolved at typecheck time so desugar doesn't re-run assignability. `Iterable` subsumes
/// `Map`/`Set` and any structurally-iterable interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForOfKind {
    Array,
    Iterator,
    Iterable,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TypedStmtKind {
    /// Function-local only; top-level `let` is in `TypedAst::globals`.
    /// `ty` is the annotation when present, else the inferred type — annotation wins on mismatch.
    Let {
        name: Ident,
        ty: Type,
        value: ExprId,
        boxed: bool,
        doc: Option<crate::DocComment>,
    },
    /// Replace the box in `ident`'s slot with a fresh one holding the same value,
    /// so closures created from here on capture a cell nothing before them shares.
    ///
    /// Only the `for` lowering emits this, for the per-iteration binding a `let`
    /// head has in JS (ECMA-262 §14.7.4.4 CreatePerIterationEnvironment): each
    /// pass gets its own copy, which is why `for (let i …) fns.push(() => i)`
    /// yields `0,1,2` rather than three views of one slot. `ident` must name a
    /// *boxed* binding — an unboxed one is captured by value, so there is
    /// nothing to separate.
    ReboxLocal {
        ident: Ident,
        ty: Type,
    },
    /// Captured `const` bindings are copied into the closure env (no `boxed` field).
    /// Function-local only — see `Let`.
    Const {
        name: Ident,
        ty: Type,
        value: ExprId,
        doc: Option<crate::DocComment>,
    },
    If {
        condition: ExprId,
        then_block: StmtId,
        else_block: Option<StmtId>,
    },
    While {
        condition: ExprId,
        body: StmtId,
    },
    /// Pre-desugar; codegen never sees this variant.
    For {
        init: Option<StmtId>,
        condition: Option<ExprId>,
        update: Option<StmtId>,
        body: StmtId,
    },
    /// Pre-desugar; codegen never sees this variant. `kind` lets desugar dispatch
    /// without re-running assignability.
    ForOf {
        binding_kind: BindingKind,
        name: Ident,
        element_ty: Type,
        iter: ExprId,
        body: StmtId,
        kind: ForOfKind,
    },
    /// Pre-desugar; lowers to a `while`-true whose head tests `cond` on every
    /// pass but the first, so a `continue` still reaches the test.
    DoWhile {
        body: StmtId,
        condition: ExprId,
    },
    /// Not lowered to if/else — first-class through desugar and codegen.
    /// `discriminant_ty` drives dispatch strategy. Case bodies are wrapped in
    /// `NarrowRegion` by the inferer for discriminated-union narrowing.
    Switch {
        discriminant: ExprId,
        discriminant_ty: Type,
        cases: Vec<TypedSwitchCase>,
        default: Option<StmtId>,
    },
    Break,
    /// Desugar injects the update step before the jump in `for`/`for-of`.
    /// A switch frame on the loop-contexts stack is skipped so `continue` inside
    /// `switch` reaches the enclosing loop.
    Continue,
    Return(Option<ExprId>),
    /// `throw <expr>`. Value must be `Error`; diverges in control-flow analysis.
    Throw {
        value: ExprId,
    },
    /// `catches` dispatch in declaration order on the thrown value's nominal
    /// class; an unmatched error re-raises. Codegen duplicates `finally` on
    /// normal/caught/uncaught paths.
    Try {
        body: StmtId,
        catches: Vec<TypedCatchClause>,
        finally: Option<StmtId>,
    },
    Expr(ExprId),
    Block(Vec<StmtId>),
    /// Function-scope binding assignment. `target_ty` is the slot's declared type;
    /// codegen coerces primitives into ref-typed slots (e.g. `number | null` boxes f64).
    AssignLocal {
        ident: Ident,
        target_ty: Type,
        value: ExprId,
        boxed: bool,
        /// When set, codegen materializes a shadow Wasm local of this narrowed type
        /// alongside the slot write. Subsequent `LocalNarrowRef` reads resolve to the
        /// shadow; it disappears at block exit.
        narrowed_shadow_ty: Option<Type>,
    },
    /// `const` and function targets are rejected. `target_ty` mirrors `AssignLocal.target_ty` for codegen coercion.
    AssignGlobal {
        ident: Ident,
        mangled: MangledName,
        target_ty: Type,
        value: ExprId,
    },
    AssignField {
        receiver: ExprId,
        name: Ident,
        value: ExprId,
    },
    /// `elem_ty` is carried so codegen picks the boxing path without re-walking
    /// the receiver's `Type`.
    AssignIndex {
        receiver: ExprId,
        index: ExprId,
        value: ExprId,
        elem_ty: Type,
    },
    /// Wraps then/else blocks and switch case bodies. `path` is for diagnostics; codegen
    /// consults only `source`, `cast_info`, `binding`, and `body`.
    NarrowRegion {
        path: ReferencePath,
        source: ExprId,
        binding: Ident,
        cast_info: CastInfo,
        body: StmtId,
    },
}

/// `ty` is the binding's class type — the root `Error` class for untyped
/// `catch (e)`, or the annotated `Error` subclass (which filters at runtime);
/// `boxed` mirrors `Let.boxed` from the Capture pass.
#[derive(Clone, Debug, PartialEq)]
pub struct TypedCatchClause {
    pub binding: Ident,
    pub ty: Type,
    pub body: StmtId,
    pub boxed: bool,
    pub span: Span,
}

/// Multiple entries in `values` when consecutive `case` labels share a body.
/// `body` is wrapped in `NarrowRegion` by the inferer for discriminated unions.
#[derive(Clone, Debug, PartialEq)]
pub struct TypedSwitchCase {
    pub values: Vec<TypedSwitchValue>,
    pub body: StmtId,
    pub span: Span,
}

impl TypedSwitchCase {
    /// The run-time comparisons of the clause's labels that aren't literals, which
    /// run before any clause body does.
    pub fn label_comparisons(&self) -> impl Iterator<Item = ExprId> + '_ {
        self.values.iter().filter_map(|value| match value {
            TypedSwitchValue::Expr { comparison, .. } => Some(*comparison),
            _ => None,
        })
    }
}

/// A `case` label. `span` anchors fallthrough and duplicate-case diagnostics.
#[derive(Clone, Debug, PartialEq)]
pub enum TypedSwitchValue {
    /// A label that isn't a literal, such as `case one:`, compared at run time.
    /// Walkers visit `comparison`, which holds `label`.
    Expr {
        label: ExprId,
        /// `discriminant === label`. The discriminant is read from the
        /// temporary the inferer binds it to, so the label can't re-run it.
        comparison: ExprId,
        /// The one value the label can have, when its type is a single literal:
        /// it then counts toward exhaustiveness as a literal label would.
        literal: Option<crate::typechecker::infer::narrowing::LiteralValue>,
        span: Span,
    },
    String {
        value: String,
        span: Span,
    },
    Number {
        value: f64,
        span: Span,
    },
    Boolean {
        value: bool,
        span: Span,
    },
    Null {
        span: Span,
    },
    /// `value` is the lowered runtime representation.
    Enum {
        enum_name: MangledName,
        member: Ident,
        value: EnumVariantPayload,
        span: Span,
    },
}

/// Distinct from the top-level `Number`/`String` variants so the inferer can attach enum-identity diagnostics.
#[derive(Clone, Debug, PartialEq)]
pub enum EnumVariantPayload {
    Number(f64),
    String(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypedParam {
    pub name: Ident,
    pub ty: Type,
    /// Set by Capture when an inner closure captures this param. The Wasm signature
    /// is unaffected — codegen emits a body prologue that wraps the arg into
    /// `(ref $box T)` and shadows it; all subsequent reads/writes go through the box.
    pub boxed: bool,
    /// Callee sees `T[]`; call sites pre-pack trailing args. Only valid on the last param.
    pub rest: bool,
    /// Resolved default for an omitted argument, carried so `PackageDeclaration`
    /// can export it — without it, cross-module calls can't omit the arg.
    pub default: Option<crate::DefaultValue>,
}

#[derive(Default, Clone, Debug)]
pub struct TypedAst {
    /// Receiver types for ordinary function expressions; arrows capture their receiver.
    pub closure_this: std::collections::BTreeMap<ExprId, Type>,
    /// Named function-expression bindings, scoped to their closure body.
    pub closure_names: std::collections::BTreeMap<ExprId, Ident>,
    /// The closures nested function declarations become, by declared name, so
    /// a diagnostic about one can name it as the function it is.
    pub nested_function_names: std::collections::BTreeMap<ExprId, Ident>,
    /// Closures with empty bodies that hold a nested function's binding until
    /// its real closure is assigned. The typechecker rejects every use before
    /// then, so they are never called: a non-void one traps if it is.
    pub placeholder_closures: std::collections::BTreeSet<ExprId>,
    /// Arguments before omitted defaults and rest packing, keyed by call span.
    pub authored_arguments: std::collections::BTreeMap<(u32, u32, u32), Vec<ExprId>>,
    /// Authored expression types retained by runtime-value lowering for member
    /// selection. Physical slot types live on the lowered expressions.
    pub runtime_source_types: std::collections::BTreeMap<ExprId, Type>,
    pub runtime_chain_types: std::collections::BTreeMap<ExprId, Vec<Type>>,
    /// For each spread source read by name, the fields its value is checked
    /// against: every field one of its object types names, with the union of
    /// the types they give it, an index signature's value type included.
    pub spread_mask_fields:
        std::collections::BTreeMap<ExprId, std::collections::BTreeMap<String, Type>>,
    /// The discriminants of the `switch`es without a `default` whose cases the
    /// typechecker found match every value the discriminant can hold.
    pub exhaustive_switches: std::collections::BTreeSet<ExprId>,
    /// Module name used for mangling. Defaults to `USER_PACKAGE` (`"main"`).
    pub package_name: String,
    exprs: Vec<TypedExpr>,
    stmts: Vec<TypedStmt>,
    /// Slot definitions only; initializers are `AssignGlobal` in `top_level_statements`.
    pub globals: Vec<TypedGlobal>,
    /// The globals code can rebind after the package loads that this package
    /// declares or imports, each with how the source names it: a module `let`,
    /// its own or one another package exports, and a static field not declared
    /// `readonly`, as `Class.field`. Every static field is a `GlobalKind::Const`
    /// global, so `globals` alone cannot tell.
    pub rebindable_globals: std::collections::BTreeMap<MangledName, String>,
    /// Top-level function declarations. Hoisted — codegen doesn't depend on source order.
    pub functions: Vec<TypedFunction>,
    /// Source-order `_start` body — currently one `AssignGlobal` per global initializer.
    pub top_level_statements: Vec<StmtId>,
    /// Type declarations (`interface`, `enum`) — no Wasm representation, no body.
    /// Stored here so post-inference passes see member spans/types without re-exposing
    /// the inferer's `TypeNamespace`.
    pub types: Vec<TypedTypeDecl>,
    /// Distinct anonymous shapes (`Object`, `Array`, `Union`) reachable from the module.
    /// Built at end of inference; consumers read this instead of re-walking the AST.
    pub shapes: Vec<crate::Shape>,
    /// Public surface: one entry per `export`-marked declaration. Single source of
    /// truth for visibility — `TypedFunction`/`TypedGlobal` carry no `is_exported`
    /// flag. Built but not yet consumed for surface projection in single-file mode.
    pub exports: Vec<ExportEntry>,
    /// External package names that were resolved from explicit imports in this
    /// module. This is deliberately package-level, not symbol-level: embedders
    /// use it to install only the package modules the typed program imports.
    pub imported_packages: std::collections::BTreeSet<String>,
    /// Concrete runtime validators resolved while inference still has access to
    /// interface declarations and generic substitutions. Codegen consults this
    /// for narrowed reads instead of re-deriving type structure.
    pub runtime_type_tests: std::collections::BTreeMap<Type, FieldNarrowingTest>,
    /// Substituted data fields for generic-class runtime validation.
    pub runtime_class_fields: std::collections::BTreeMap<Type, Type>,
    pub runtime_class_parameters: std::collections::BTreeMap<MangledName, Vec<String>>,
    pub runtime_class_contexts: std::collections::BTreeMap<Type, Vec<InstanceTypeContext>>,
    pub runtime_field_guards: std::collections::BTreeMap<Type, Vec<InstantiatedFieldGuard>>,
}

#[derive(Clone, Debug)]
pub struct InstanceTypeContext {
    pub declaration: MangledName,
    pub args: Vec<Type>,
}

/// A public export: maps a package-public mangled name onto the internal symbol
/// it refers to. In single-file packages `public_name == target`.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportEntry {
    pub public_name: MangledName,
    pub target: MangledName,
    pub kind: ExportKind,
    /// Span of the `export` keyword (or re-export statement) that produced this entry.
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportKind {
    Function,
    Global,
    Type,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypedGlobal {
    pub name: Ident,
    pub mangled_name: MangledName,
    pub ty: Type,
    pub kind: GlobalKind,
    pub doc: Option<crate::DocComment>,
    pub span: Span,
}

/// Codegen treats both identically at the Wasm-global level; the inferer enforces the `const` write diagnostic.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GlobalKind {
    Let,
    Const,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypedFunction {
    pub name: Ident,
    pub mangled_name: MangledName,
    /// Body is type-checked under `Type::GenericParam` instantiations then erased
    /// to `Type::TypeVar` for the external view.
    pub generics: Vec<String>,
    pub params: Vec<TypedParam>,
    pub return_type: Type,
    /// Type guard predicate. When set, `return_type` is always `Type::Boolean`.
    pub type_predicate: Option<crate::TypePredicate>,
    pub body: StmtId,
    pub doc: Option<crate::DocComment>,
    pub span: Span,
}

/// Enums split by representation kind so the typed AST can't represent a mixed-kind enum.
#[derive(Clone, Debug, PartialEq)]
pub enum TypedTypeDecl {
    Interface(TypedInterfaceDecl),
    Class(TypedClassDecl),
    NumberEnum(TypedNumberEnumDecl),
    StringEnum(TypedStringEnumDecl),
    Alias(TypedTypeAliasDecl),
}

/// A typechecked class. Method/constructor bodies are checked in the class-body
/// pass; codegen (SUB-480+) consumes the field layout, body IDs, and signatures.
#[derive(Clone, Debug, PartialEq)]
pub struct TypedClassDecl {
    pub name: Ident,
    pub fields: Vec<TypedClassField>,
    /// Static member visibility retained for analyses that run after package
    /// declarations have been reduced to their runtime surface.
    pub static_methods: BTreeMap<String, crate::Visibility>,
    /// Static field signatures are lowered to globals, but their source-level
    /// visibility and callable types remain relevant to public-surface analysis.
    pub static_fields: BTreeMap<String, crate::FieldSig>,
    pub constructor: Option<TypedClassConstructor>,
    /// The signature a class with no `constructor` of its own exposes, taken
    /// from the nearest ancestor that declares one with the `extends` clause's
    /// type arguments already substituted. Empty when `constructor` is `Some`.
    ///
    /// This is what the package declaration exports and what a cross-package
    /// consumer imports, so the emitted constructor must use it rather than
    /// re-deriving the parent's unsubstituted parameters — the two disagree the
    /// moment a subclass fixes a generic parent's type argument.
    pub inherited_ctor_params: Vec<TypedParam>,
    pub methods: Vec<TypedClassMethod>,
    /// Accessor functions (`get`/`set`), one entry each — getter and setter are
    /// symmetric, neither is required (a property may be get-only, set-only, or
    /// both). They are *not* in `methods`; codegen synthesizes the vtable method
    /// per entry (see `codegen::classes`).
    pub accessors: Vec<TypedClassAccessor>,
    pub extends: Option<crate::MangledName>,
    pub implements: Vec<crate::MangledName>,
    pub mangled_name: crate::MangledName,
    pub doc: Option<crate::DocComment>,
}

/// One accessor function. Getter and setter are independent: the read type
/// (`ret_ty`) and write type (`param.ty`) need not match (TS 4.3+).
#[derive(Clone, Debug, PartialEq)]
pub enum TypedClassAccessor {
    Getter {
        name: Ident,
        ret_ty: Type,
        visibility: crate::Visibility,
        body: StmtId,
    },
    Setter {
        name: Ident,
        /// The setter parameter; `param.ty` is the property's write type.
        param: TypedParam,
        visibility: crate::Visibility,
        body: StmtId,
    },
}

impl TypedClassAccessor {
    pub fn name(&self) -> &Ident {
        match self {
            TypedClassAccessor::Getter { name, .. } | TypedClassAccessor::Setter { name, .. } => {
                name
            }
        }
    }

    pub fn body(&self) -> StmtId {
        match self {
            TypedClassAccessor::Getter { body, .. } | TypedClassAccessor::Setter { body, .. } => {
                *body
            }
        }
    }

    pub fn visibility(&self) -> crate::Visibility {
        match self {
            TypedClassAccessor::Getter { visibility, .. }
            | TypedClassAccessor::Setter { visibility, .. } => *visibility,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypedClassField {
    pub name: Ident,
    pub ty: Type,
    pub visibility: crate::Visibility,
    pub readonly: bool,
    pub optional: bool,
    pub initializer: Option<ExprId>,
    /// `true` for a parameter property (`constructor(public x: T)`): the field is
    /// assigned from the constructor parameter, so it's exempt from the
    /// definite-assignment check.
    pub auto_assigned: bool,
    /// Set when this declaration *narrows* an inherited one. See
    /// [`FieldNarrowingCheck`]. Boxed because every class field carries this and
    /// almost none of them narrow.
    pub narrowing_check: Option<Box<FieldNarrowingCheck>>,
    pub doc: Option<crate::DocComment>,
}

/// A read guard for a field whose subclass declaration narrows the inherited
/// one. The two declarations share one storage slot, so a write that goes
/// through the parent's — an inherited method, a parent-typed reference, the
/// parent's constructor — can leave the slot holding a value the subclass's
/// declaration does not admit. The language accepts the narrowing anyway
/// (spec.md §Classes, matching TypeScript), so the read is what has to check.
///
/// Without this the read's bare `ref.cast` raises an uncatchable `cast failure`
/// naming nothing the author wrote; with it, the read runs `test` first —
/// presence for a narrowing that only strips `null`, structural conformance
/// otherwise — and throws `message` as a catchable `TypeError`.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FieldNarrowingCheck {
    #[serde(default)]
    pub declaration: Option<MangledName>,
    pub test: FieldNarrowingTest,
    /// A concrete declaration type whose ancestor alternatives need only a
    /// presence or representation check. Other read types still validate fully.
    #[serde(default)]
    pub minimal_test_target: Option<Type>,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InstantiatedFieldGuard {
    pub field: String,
    pub target: Type,
    pub check: FieldNarrowingCheck,
}

/// What a *redeclared* field's read guard verifies before it casts. The
/// typechecker selects the most precise test codegen can lower, falling back to
/// a read-time substituted test for erased class parameters.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum FieldNarrowingTest {
    /// The two declarations differ only in admitting `null`, so presence is the
    /// whole check. Worth its own case: the structural walk below is O(size of
    /// the stored value), and this is both the commonest narrowing and the one
    /// where walking proves nothing. It also has no shape to lower, so it guards
    /// types `Shape` cannot — a recursive one, an interface with methods.
    ///
    /// Whether the test actually runs is settled at the read, not here: an
    /// erased type parameter's `v: T` is `string | null` at `Sub<string | null>`,
    /// where a `null` is legal, and `string` at `Sub<string>`, where it is not.
    /// `emit_narrowed_field_read` asks the substituted read type and skips the
    /// test when it admits `null`.
    NonNull,
    /// Structural conformance to this shape, which is restricted to what
    /// `cast_check::emit_structural_test` can lower.
    Shape(Type),
    /// Runtime member check for an interface. Methods are read through the
    /// object-shape getter (which can surface class vtable slots), while data
    /// properties use the ordinary accessor-aware conformance walk. The method
    /// set is empty for a data-only interface.
    Interface(InterfaceNarrowingTest),
    /// The declaration contains an erased class type parameter, so its concrete
    /// runtime shape is available only at the read. Codegen tests the
    /// substituted `result_ty` rather than silently falling back to a bare cast.
    Substituted,
    /// Only the target Wasm representation can be established. This still
    /// makes the following cast safe and turns mismatches into `TypeError`.
    Representation,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct InterfaceNarrowingTest {
    pub index: Option<crate::IndexSignature>,
    pub members: std::collections::BTreeMap<String, crate::ObjectField>,
    pub methods: std::collections::BTreeSet<String>,
    pub non_shape_carriers: std::collections::BTreeSet<InterfaceCarrier>,
    /// Whether an ordinary `$ObjectShape` may satisfy the interface. Direct
    /// host dispatch requires its canonical carrier; vtable interfaces remain
    /// structurally implementable by user objects.
    #[serde(default = "interface_shape_allowed_default")]
    pub shape_allowed: bool,
    pub nullable: bool,
}

fn interface_shape_allowed_default() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum InterfaceCarrier {
    Number,
    Boolean,
    String,
    BigInt,
    Uint8Array,
    ArrayAny,
    Array(Type),
    MapAny,
    Map(Type, Type),
    SetAny,
    Set(Type),
    RegExp,
    RegExpMatch,
    TemporalInstant,
    TemporalDuration,
    TemporalZonedDateTime,
    TemporalPlainDate,
    TemporalPlainTime,
    TemporalPlainDateTime,
    TemporalPlainYearMonth,
    TemporalPlainMonthDay,
    ObjectShape,
    Url,
    FsStat,
    FsPeek,
    FsDirEntry,
    FsInfo,
    FsMountInfo,
    FsFileWriter,
    HttpResponse,
    HttpDownloadResult,
    SessionEntry,
    SessionPage,
}

/// Whether codegen can validate every value admitted by `ty` with
/// `cast_check::emit_structural_test`. Keeping the allowlist beside the
/// serialized narrowing-test model gives typechecking and codegen one answer.
pub(crate) fn runtime_type_is_testable(ty: &Type) -> bool {
    runtime_type_is_testable_inner(ty, false)
}

/// Field redeclaration guards can root generated validators at recursive alias
/// or interface back-edges. General expression descriptors cannot: polymorphic
/// recursion can grow its type arguments forever while enumerating every
/// expression type.
pub(crate) fn field_runtime_type_is_testable(ty: &Type) -> bool {
    runtime_type_is_testable_inner(ty, true)
}

fn runtime_type_is_testable_inner(ty: &Type, allow_recursive_ref: bool) -> bool {
    match ty.peel() {
        Type::Null
        | Type::Number
        | Type::NumberLiteral(_)
        | Type::Boolean
        | Type::BooleanLiteral(_)
        | Type::String
        | Type::StringLiteral(_)
        | Type::BigInt
        | Type::Uint8Array
        | Type::Unknown
        | Type::Function { .. }
        | Type::NumberEnum { .. }
        | Type::StringEnum { .. }
        | Type::ClassRef { .. } => true,
        Type::AliasRef { .. } | Type::InterfaceRef { .. } => allow_recursive_ref,
        Type::Array(elem) => runtime_type_is_testable_inner(elem, allow_recursive_ref),
        Type::Tuple(elems) | Type::Union(elems) => elems
            .iter()
            .all(|elem| runtime_type_is_testable_inner(elem, allow_recursive_ref)),
        Type::Object { fields, index } => {
            index
                .as_ref()
                .is_none_or(|i| runtime_type_is_testable_inner(&i.value, allow_recursive_ref))
                && fields
                    .values()
                    .all(|field| runtime_type_is_testable_inner(&field.ty, allow_recursive_ref))
        }
        _ => false,
    }
}

impl TypedClassDecl {
    /// The constructor signature this class exposes: its own if it declares
    /// one, otherwise the inherited signature. Callers should prefer this over
    /// reading either field directly — `inherited_ctor_params` is meaningful
    /// only when `constructor` is `None`.
    pub fn effective_ctor_params(&self) -> &[TypedParam] {
        match &self.constructor {
            Some(c) => &c.params,
            None => &self.inherited_ctor_params,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypedClassConstructor {
    pub doc: Option<Box<crate::DocComment>>,
    pub span: Span,
    pub params: Vec<TypedParam>,
    pub body: StmtId,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypedClassMethod {
    pub name: Ident,
    pub generics: Vec<String>,
    pub params: Vec<TypedParam>,
    pub return_type: Type,
    pub body: StmtId,
    pub visibility: crate::Visibility,
    pub doc: Option<crate::DocComment>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypedInterfaceDecl {
    /// Complete property names, including inherited properties, for payload validation.
    pub property_names: std::collections::BTreeSet<String>,
    pub index: Option<crate::IndexSignature>,
    pub name: Ident,
    pub generics: Vec<String>,
    pub members: Vec<TypedInterfaceMember>,
    pub doc: Option<crate::DocComment>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TypedInterfaceMember {
    Method {
        name: Ident,
        generics: Vec<String>,
        params: Vec<TypedParam>,
        return_type: Type,
        doc: Option<crate::DocComment>,
    },
    Property {
        name: Ident,
        ty: Type,
        readonly: bool,
        /// Reads widen to `T | null`; may be omitted at construction.
        optional: bool,
        doc: Option<crate::DocComment>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypedNumberEnumDecl {
    pub name: Ident,
    pub members: Vec<TypedNumberEnumMember>,
    pub doc: Option<crate::DocComment>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypedNumberEnumMember {
    pub name: Ident,
    pub value: f64,
    pub doc: Option<crate::DocComment>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypedStringEnumDecl {
    pub name: Ident,
    pub members: Vec<TypedStringEnumMember>,
    pub doc: Option<crate::DocComment>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypedStringEnumMember {
    pub name: Ident,
    pub value: String,
    pub doc: Option<crate::DocComment>,
}

/// `ty` is fully resolved; `Type::TypeVar(name)` placeholders correspond to `generics` entries.
#[derive(Clone, Debug, PartialEq)]
pub struct TypedTypeAliasDecl {
    pub name: Ident,
    pub generics: Vec<String>,
    pub ty: Type,
    pub doc: Option<crate::DocComment>,
}

impl TypedAst {
    pub(crate) fn has_string_index(&self, ty: &Type) -> bool {
        match ty.peel() {
            Type::Object { index, .. } => index.is_some(),
            Type::Union(members) => members.iter().any(|member| self.has_string_index(member)),
            Type::InterfaceRef { .. } => matches!(
                self.runtime_type_tests.get(ty.peel()),
                Some(FieldNarrowingTest::Interface(interface)) if interface.index.is_some()
            ),
            _ => false,
        }
    }

    pub fn new() -> Self {
        Self {
            package_name: crate::mangle::USER_PACKAGE.to_string(),
            ..Self::default()
        }
    }

    pub fn with_package(package_name: impl Into<String>) -> Self {
        Self {
            package_name: package_name.into(),
            ..Self::default()
        }
    }

    pub fn record_authored_arguments(&mut self, span: Span, args: Vec<ExprId>) {
        self.authored_arguments
            .insert((span.file.0, span.start, span.end), args);
    }

    pub fn authored_call_arguments(&self, span: Span) -> Option<&Vec<ExprId>> {
        self.authored_arguments
            .get(&(span.file.0, span.start, span.end))
    }

    /// Checked allocation; failure leaves the arena unchanged.
    pub fn try_push_expr(&mut self, expr: TypedExpr) -> Result<ExprId, ArenaError> {
        arena::push(&mut self.exprs, expr, ArenaKind::TypedExpressions).map(ExprId)
    }

    /// Checked allocation; failure leaves the arena unchanged.
    pub fn try_push_stmt(&mut self, stmt: TypedStmt) -> Result<StmtId, ArenaError> {
        arena::push(&mut self.stmts, stmt, ArenaKind::TypedStatements).map(StmtId)
    }

    pub fn try_expr(&self, id: ExprId) -> Result<&TypedExpr, ArenaError> {
        arena::get(&self.exprs, id.0, ArenaKind::TypedExpressions)
    }

    pub fn try_stmt(&self, id: StmtId) -> Result<&TypedStmt, ArenaError> {
        arena::get(&self.stmts, id.0, ArenaKind::TypedStatements)
    }

    pub fn try_expr_mut(&mut self, id: ExprId) -> Result<&mut TypedExpr, ArenaError> {
        arena::get_mut(&mut self.exprs, id.0, ArenaKind::TypedExpressions)
    }

    pub fn try_stmt_mut(&mut self, id: StmtId) -> Result<&mut TypedStmt, ArenaError> {
        arena::get_mut(&mut self.stmts, id.0, ArenaKind::TypedStatements)
    }

    /// Snapshot of allocated expression IDs, usable while appending new nodes.
    pub fn expr_ids(&self) -> Result<impl DoubleEndedIterator<Item = ExprId> + use<>, ArenaError> {
        Ok(arena::ids(self.exprs.len(), ArenaKind::TypedExpressions)?.map(ExprId))
    }

    /// Snapshot of allocated statement IDs, usable while appending new nodes.
    pub fn stmt_ids(&self) -> Result<impl DoubleEndedIterator<Item = StmtId> + use<>, ArenaError> {
        Ok(arena::ids(self.stmts.len(), ArenaKind::TypedStatements)?.map(StmtId))
    }

    /// Body `StmtId`s of every class constructor + method — additional codegen
    /// roots alongside `functions` (the string/bigint/closure/box collection
    /// passes must walk these too).
    pub fn class_body_roots(&self) -> Vec<StmtId> {
        let mut roots = Vec::new();
        for decl in &self.types {
            if let TypedTypeDecl::Class(c) = decl {
                if let Some(ctor) = &c.constructor {
                    roots.push(ctor.body);
                }
                roots.extend(c.methods.iter().map(|m| m.body));
                roots.extend(c.accessors.iter().map(TypedClassAccessor::body));
            }
        }
        roots
    }

    /// Field-initializer expressions across all classes — `expr` roots that live
    /// outside any statement body, so analysis/erasure passes must visit them too.
    pub fn class_field_initializers(&self) -> Vec<ExprId> {
        let mut inits = Vec::new();
        for decl in &self.types {
            if let TypedTypeDecl::Class(c) = decl {
                inits.extend(c.fields.iter().filter_map(|f| f.initializer));
            }
        }
        inits
    }

    pub fn source_type(&self, id: ExprId) -> Result<&Type, ArenaError> {
        let expr = self.try_expr(id)?;
        Ok(self.runtime_source_types.get(&id).unwrap_or(&expr.ty))
    }

    /// Used by post-inference passes to iterate just-pushed IDs (e.g. GenericParam erasure).
    pub fn exprs_len(&self) -> usize {
        self.exprs.len()
    }

    pub fn stmts_len(&self) -> usize {
        self.stmts.len()
    }

    /// Whether evaluating `id` can be skipped without changing what the program
    /// does. Deliberately a short whitelist of leaves: everything else — a call,
    /// a field read that may hit an accessor, an operator that may throw — says
    /// `false`, so a caller that folds an expression's value away and relies on
    /// this to decide whether to keep the computation errs toward keeping it.
    pub fn is_effect_free(&self, id: ExprId) -> Result<bool, ArenaError> {
        Ok(matches!(
            self.try_expr(id)?.kind,
            TypedExprKind::Number(_)
                | TypedExprKind::BigInt(_)
                | TypedExprKind::String(_)
                | TypedExprKind::Boolean(_)
                | TypedExprKind::Null
                | TypedExprKind::This
                | TypedExprKind::Regex { .. }
                | TypedExprKind::LocalRef { .. }
                | TypedExprKind::LocalNarrowRef { .. }
                | TypedExprKind::GlobalRef { .. }
                | TypedExprKind::FunctionRef { .. }
                | TypedExprKind::NumberEnumMember { .. }
                | TypedExprKind::StringEnumMember { .. }
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{TypedAst, TypedExpr, TypedExprKind, TypedParam, TypedStmt, TypedStmtKind};
    use crate::{BinOp, Ident, Span, Type, UnOp};

    #[test]
    fn arena_round_trip_for_typed_exprs() {
        let mut ast = TypedAst::new();
        let id = ast
            .try_push_expr(TypedExpr {
                kind: TypedExprKind::Number(42.0),
                span: Span::new(crate::FileId(0), 0, 2).unwrap(),
                ty: Type::Number,
            })
            .unwrap();
        assert_eq!(id.0, 0);
        let e = ast.try_expr(id).unwrap();
        assert_eq!(e.kind, TypedExprKind::Number(42.0));
        assert_eq!(e.span, Span::new(crate::FileId(0), 0, 2).unwrap());
        assert_eq!(e.ty, Type::Number);
    }

    #[test]
    fn arena_round_trip_for_typed_stmts() {
        let mut ast = TypedAst::new();
        let expr_id = ast
            .try_push_expr(TypedExpr {
                kind: TypedExprKind::Boolean(true),
                span: Span::new(crate::FileId(0), 0, 4).unwrap(),
                ty: Type::Boolean,
            })
            .unwrap();
        let stmt_id = ast
            .try_push_stmt(TypedStmt {
                kind: TypedStmtKind::Expr(expr_id),
                span: Span::new(crate::FileId(0), 0, 5).unwrap(),
            })
            .unwrap();
        assert_eq!(stmt_id.0, 0);
        let s = ast.try_stmt(stmt_id).unwrap();
        assert_eq!(s.span, Span::new(crate::FileId(0), 0, 5).unwrap());
        assert!(matches!(s.kind, TypedStmtKind::Expr(_)));
    }

    #[test]
    fn build_typed_binary_tree() {
        let mut ast = TypedAst::new();
        let lhs = ast
            .try_push_expr(TypedExpr {
                kind: TypedExprKind::Number(1.0),
                span: Span::new(crate::FileId(0), 0, 1).unwrap(),
                ty: Type::Number,
            })
            .unwrap();
        let rhs = ast
            .try_push_expr(TypedExpr {
                kind: TypedExprKind::Number(2.0),
                span: Span::new(crate::FileId(0), 4, 5).unwrap(),
                ty: Type::Number,
            })
            .unwrap();
        let sum = ast
            .try_push_expr(TypedExpr {
                kind: TypedExprKind::Binary {
                    op: BinOp::Add,
                    lhs,
                    rhs,
                },
                span: Span::new(crate::FileId(0), 0, 5).unwrap(),
                ty: Type::Number,
            })
            .unwrap();

        let outer = ast.try_expr(sum).unwrap();
        assert_eq!(outer.ty, Type::Number);
        let TypedExprKind::Binary { op, lhs, rhs } = outer.kind else {
            panic!("expected Binary");
        };
        assert_eq!(op, BinOp::Add);
        assert_eq!(ast.try_expr(lhs).unwrap().ty, Type::Number);
        assert_eq!(ast.try_expr(rhs).unwrap().ty, Type::Number);
    }

    #[test]
    fn build_typed_function_with_return() {
        let mut ast = TypedAst::new();
        let one = ast
            .try_push_expr(TypedExpr {
                kind: TypedExprKind::Number(1.0),
                span: Span::new(crate::FileId(0), 31, 32).unwrap(),
                ty: Type::Number,
            })
            .unwrap();
        let ret = ast
            .try_push_stmt(TypedStmt {
                kind: TypedStmtKind::Return(Some(one)),
                span: Span::new(crate::FileId(0), 24, 33).unwrap(),
            })
            .unwrap();
        let body = ast
            .try_push_stmt(TypedStmt {
                kind: TypedStmtKind::Block(vec![ret]),
                span: Span::new(crate::FileId(0), 22, 35).unwrap(),
            })
            .unwrap();
        ast.functions.push(crate::TypedFunction {
            name: Ident {
                name: "f".to_string(),
                span: Span::new(crate::FileId(0), 9, 10).unwrap(),
            },
            mangled_name: crate::mangle::package_symbol("main", "f"),
            generics: vec![],
            params: vec![],
            return_type: Type::Number,
            type_predicate: None,
            body,
            doc: None,
            span: Span::new(crate::FileId(0), 0, 35).unwrap(),
        });

        let f = &ast.functions[0];
        assert_eq!(f.return_type, Type::Number);
        assert!(matches!(
            ast.try_stmt(f.body).unwrap().kind,
            TypedStmtKind::Block(_)
        ));
    }

    #[test]
    fn clone_and_equality() {
        let kind = TypedExprKind::Unary {
            op: UnOp::Neg,
            operand: crate::ExprId(0),
        };
        assert_eq!(kind.clone(), kind);
    }

    #[test]
    fn typed_param_construction() {
        let p = TypedParam {
            name: Ident {
                name: "param".to_string(),
                span: Span::new(crate::FileId(0), 10, 15).unwrap(),
            },
            ty: Type::String,
            boxed: false,
            rest: false,
            default: None,
        };
        assert_eq!(p.name.name, "param");
        assert_eq!(p.name.span, Span::new(crate::FileId(0), 10, 15).unwrap());
        assert_eq!(p.ty, Type::String);
        assert!(!p.boxed);
    }

    #[test]
    fn local_ref_carries_name_and_boxed_flag() {
        let mut ast = TypedAst::new();
        let id = ast
            .try_push_expr(TypedExpr {
                kind: TypedExprKind::LocalRef {
                    ident: Ident {
                        name: "x".to_string(),
                        span: Span::new(crate::FileId(0), 0, 1).unwrap(),
                    },
                    boxed: false,
                },
                span: Span::new(crate::FileId(0), 0, 1).unwrap(),
                ty: Type::Number,
            })
            .unwrap();
        match ast.try_expr(id).unwrap().kind {
            TypedExprKind::LocalRef { ref ident, boxed } => {
                assert_eq!(ident.name, "x");
                assert_eq!(ident.span, Span::new(crate::FileId(0), 0, 1).unwrap());
                assert!(!boxed);
            }
            _ => panic!("expected LocalRef"),
        }
    }

    #[test]
    fn global_ref_carries_name_and_mangled() {
        let mut ast = TypedAst::new();
        let mangled = crate::mangle::package_symbol("main", "y");
        let id = ast
            .try_push_expr(TypedExpr {
                kind: TypedExprKind::GlobalRef {
                    mangled: mangled.clone(),
                    name: Ident {
                        name: "y".to_string(),
                        span: Span::new(crate::FileId(0), 2, 3).unwrap(),
                    },
                },
                span: Span::new(crate::FileId(0), 2, 3).unwrap(),
                ty: Type::Number,
            })
            .unwrap();
        match ast.try_expr(id).unwrap().kind {
            TypedExprKind::GlobalRef {
                ref name,
                ref mangled,
            } => {
                assert_eq!(name.name, "y");
                assert_eq!(name.span, Span::new(crate::FileId(0), 2, 3).unwrap());
                assert_eq!(mangled.as_str(), "main#y");
            }
            _ => panic!("expected GlobalRef"),
        }
    }
}
