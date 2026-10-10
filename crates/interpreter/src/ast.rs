use crate::arena::{self, ArenaError, ArenaKind};
use crate::{DocComment, Span};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExprId(pub u32);

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StmtId(pub u32);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    Number(f64),
    /// Sign-free decimal digits; sign is the enclosing `Unary { op: Neg }`. Codegen packs into limbs.
    BigInt(String),
    String(String),
    Boolean(bool),
    Null,
    Identifier(Ident),
    Binary {
        op: BinOp,
        lhs: ExprId,
        rhs: ExprId,
    },
    Unary {
        op: UnOp,
        operand: ExprId,
    },
    Call {
        callee: ExprId,
        type_args: Option<Vec<TypeAnnotation>>,
        args: Vec<ExprId>,
    },
    Paren(ExprId),
    ObjectLiteral {
        members: Vec<ObjectLiteralMember>,
    },
    ArrayLiteral {
        elements: Vec<ArrayLiteralElement>,
    },
    FieldAccess {
        receiver: ExprId,
        name: Ident,
    },
    IndexAccess {
        receiver: ExprId,
        index: ExprId,
    },
    FunctionExpression {
        name: Option<Ident>,
        this_type: Option<TypeAnnotation>,
        function: ExprId,
    },
    Arrow {
        params: Vec<ParamDecl>,
        return_type: Option<TypeAnnotation>,
        type_predicate: Option<TypePredicateAnnotation>,
        body: ArrowBody,
    },
    /// Evaluate the operand for effects and produce `undefined`.
    Void {
        operand: ExprId,
    },
    /// `typeof x` — only valid against a string-literal tag. Folded into `TypedExprKind::TypeofTag`; never appears in the typed AST.
    Typeof {
        operand: ExprId,
    },
    /// `delete o.x` — parsed so the typechecker can reject it by name rather than
    /// as a stray identifier; never appears in the typed AST.
    Delete {
        operand: ExprId,
    },
    /// `new Foo(args)`. Lowered to `TypedExprKind::MethodCall` by the typechecker; codegen never sees this variant.
    New {
        callee: ExprId,
        type_args: Option<Vec<TypeAnnotation>>,
        args: Vec<ExprId>,
    },
    /// `this` — a class receiver, a function receiver, or an arrow capture.
    /// `Span` lives on the enclosing `Expr`.
    This,
    /// `this` parsed across a receiver boundary. Keep this distinction when
    /// method shorthand is lowered to an arrow; inference owns the diagnostic.
    ThisOutsideReceiver,
    /// `super` — composes with `Call`/`FieldAccess` for `super(...)` / `super.method(...)`.
    Super,
    /// `parts.len() == exprs.len() + 1`. No-substitution `` `plain` `` is collapsed to `String` by the parser.
    TemplateLiteral {
        parts: Vec<String>,
        exprs: Vec<ExprId>,
        /// Each substitution's span, `${` through `}`, parallel to `exprs`.
        substitution_spans: Vec<Span>,
    },
    Ternary {
        cond: ExprId,
        then_: ExprId,
        else_: ExprId,
    },
    OptionalChain {
        base: ExprId,
        parts: Vec<ChainPart>,
    },
    PostfixUnary {
        op: PostfixOp,
        operand: ExprId,
    },
    /// An assignment used as a value: `b = (a = 3)`, `while ((m = next()) !== null)`.
    /// It yields the assigned value. One in statement position is parsed as an
    /// assignment statement instead. `target` is an identifier, field access, or
    /// index access; `op` is set for a compound assignment (`+=`).
    Assign {
        target: ExprId,
        op: Option<BinOp>,
        op_span: Span,
        value: ExprId,
    },
    /// `x as T` — runtime-checked, unlike TypeScript's unchecked `as`. Codegen emits `ref.test`; throws `Error` on mismatch.
    As {
        expr: ExprId,
        ty: TypeAnnotation,
    },
    /// `x instanceof Foo` — runtime nominal class test: a ref.eq walk over the
    /// vtable-singleton parent chain (docs/classes.md §9). The right-hand side
    /// names a class type, not a value.
    InstanceOf {
        value: ExprId,
        ty: TypeAnnotation,
    },
    /// Regex literal. Pattern and flags validated at compile time; `Diagnostic` on invalid pattern or flag.
    Regex {
        source: String,
        flags: String,
    },
}

/// Postfix only — prefix `++x`/`--x` is not supported.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum PostfixOp {
    Inc,
    Dec,
    NonNullAssert,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ChainPart {
    Field {
        name: Ident,
        optional: bool,
        span: Span,
    },
    Index {
        idx: ExprId,
        optional: bool,
        span: Span,
    },
    Call {
        args: Vec<ExprId>,
        type_args: Option<Vec<TypeAnnotation>>,
        optional: bool,
        span: Span,
    },
    /// A `!` continuation (`a?.b!.c`). Carries no `optional` flag: `!` asserts the
    /// value it is applied to is non-null, so it never short-circuits.
    NonNull { span: Span },
}

impl ChainPart {
    pub fn is_optional(&self) -> bool {
        match self {
            ChainPart::Field { optional, .. }
            | ChainPart::Index { optional, .. }
            | ChainPart::Call { optional, .. } => *optional,
            ChainPart::NonNull { .. } => false,
        }
    }

    /// This step with its `?.` dropped, for a receiver that can't be `null`.
    pub fn as_plain_step(mut self) -> Self {
        match &mut self {
            ChainPart::Field { optional, .. }
            | ChainPart::Index { optional, .. }
            | ChainPart::Call { optional, .. } => *optional = false,
            ChainPart::NonNull { .. } => {}
        }
        self
    }

    pub fn span(&self) -> Span {
        match self {
            ChainPart::Field { span, .. }
            | ChainPart::Index { span, .. }
            | ChainPart::Call { span, .. }
            | ChainPart::NonNull { span } => *span,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ArrowBody {
    Expr(ExprId),
    Block(StmtId),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObjectLiteralField {
    pub name: Ident,
    pub value: ExprId,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ObjectLiteralMember {
    Computed {
        key: ExprId,
        value: ExprId,
    },
    Field(ObjectLiteralField),
    Spread {
        value: ExprId,
        /// Span of the `...` token plus operand.
        span: Span,
    },
}

impl ObjectLiteralMember {
    pub fn expressions(&self) -> impl Iterator<Item = ExprId> {
        let key = match self {
            Self::Computed { key, .. } => Some(*key),
            _ => None,
        };
        key.into_iter().chain(std::iter::once(self.value()))
    }

    pub fn value(&self) -> ExprId {
        match self {
            ObjectLiteralMember::Field(f) => f.value,
            ObjectLiteralMember::Spread { value, .. }
            | ObjectLiteralMember::Computed { value, .. } => *value,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ArrayLiteralElement {
    Value(ExprId),
    Spread {
        value: ExprId,
        /// Span of the `...` token plus operand.
        span: Span,
    },
}

impl ArrayLiteralElement {
    pub fn value(&self) -> ExprId {
        match self {
            ArrayLiteralElement::Value(id) => *id,
            ArrayLiteralElement::Spread { value, .. } => *value,
        }
    }
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
    pub(crate) fn bitwise_name(self) -> Option<&'static str> {
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

#[derive(Clone, Debug, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StmtKind {
    Let {
        name: Ident,
        ty: Option<TypeAnnotation>,
        value: ExprId,
        doc: Option<DocComment>,
    },
    Const {
        name: Ident,
        ty: Option<TypeAnnotation>,
        value: ExprId,
        doc: Option<DocComment>,
    },
    /// Destructuring `let`. Eliminated by `lower_patterns` into plain `Let` stmts; never reaches the typechecker.
    LetPattern {
        binding: Binding,
        ty: Option<TypeAnnotation>,
        value: ExprId,
        doc: Option<DocComment>,
    },
    ConstPattern {
        binding: Binding,
        ty: Option<TypeAnnotation>,
        value: ExprId,
        doc: Option<DocComment>,
    },
    /// Rest binding from `const { a, ...rest } = obj` or its `let` form. `name` gets a narrowed type with `exclude` fields removed; codegen treats it like `Const` or `Let`.
    ObjectRest {
        is_const: bool,
        name: Ident,
        source: ExprId,
        exclude: Vec<Ident>,
        ty: Option<TypeAnnotation>,
        doc: Option<DocComment>,
    },
    Function {
        name: Ident,
        generics: Vec<Ident>,
        params: Vec<ParamDecl>,
        /// `None` when a type predicate is declared; exactly one of `return_type` / `type_predicate` is `Some` after parse.
        return_type: Option<TypeAnnotation>,
        type_predicate: Option<TypePredicateAnnotation>,
        body: StmtId,
        doc: Option<DocComment>,
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
    /// C-style `for` loop. `update` is a `StmtId` so an assignment there is an assignment statement.
    For {
        init: Option<StmtId>,
        condition: Option<ExprId>,
        update: Option<StmtId>,
        /// Always a `Block` — parser requires `{ … }`.
        body: StmtId,
    },
    ForOf {
        binding_kind: BindingKind,
        name: Ident,
        ty: Option<TypeAnnotation>,
        iter: ExprId,
        /// Always a `Block` — parser requires `{ … }`.
        body: StmtId,
    },
    /// Destructuring `for-of` head. Eliminated by `lower_patterns`; never reaches the typechecker.
    ForOfPattern {
        binding_kind: BindingKind,
        binding: Binding,
        ty: Option<TypeAnnotation>,
        iter: ExprId,
        /// Always a `Block` — parser requires `{ … }`.
        body: StmtId,
    },
    DoWhile {
        /// Always a `Block` — parser requires `{ … }`.
        body: StmtId,
        condition: ExprId,
    },
    /// No fallthrough — typechecker rejects a case body that doesn't end in `break` or `return`.
    Switch {
        discriminant: ExprId,
        cases: Vec<SwitchCase>,
        default: Option<SwitchDefault>,
    },
    Break,
    Continue,
    Return(Option<ExprId>),
    Throw {
        value: ExprId,
    },
    /// `catches` may be empty and `finally` optional, but at least one must appear.
    /// Multiple `catch` clauses dispatch in declaration order on the thrown
    /// value's nominal class; the typechecker rejects unreachable arms.
    Try {
        body: StmtId,
        catches: Vec<CatchClause>,
        finally: Option<StmtId>,
    },
    Expr(ExprId),
    Block(Vec<StmtId>),
    Assign {
        target: Ident,
        value: ExprId,
    },
    AssignField {
        receiver: ExprId,
        field_name: Ident,
        value: ExprId,
    },
    AssignIndex {
        receiver: ExprId,
        index: ExprId,
        value: ExprId,
    },
    /// `target += value;` etc. Lowered to `Assign` with a synthesized `Binary`; `op_span` anchors infer diagnostics.
    CompoundAssign {
        target: Ident,
        op: BinOp,
        op_span: Span,
        value: ExprId,
    },
    CompoundAssignField {
        receiver: ExprId,
        field_name: Ident,
        op: BinOp,
        op_span: Span,
        value: ExprId,
    },
    CompoundAssignIndex {
        receiver: ExprId,
        index: ExprId,
        op: BinOp,
        op_span: Span,
        value: ExprId,
    },
    /// Interface declaration. Single declaration site per name; no reopening.
    InterfaceDecl {
        name: Ident,
        generics: Vec<Ident>,
        extends: Vec<TypeAnnotation>,
        members: Vec<InterfaceMember>,
        doc: Option<DocComment>,
    },
    /// Class declaration. Single inheritance via `extends`, structural `implements`.
    /// `generics` is parsed and stored for forward-compat; generic classes are not yet
    /// consumed downstream (classes.md non-goals).
    ClassDecl {
        name: Ident,
        generics: Vec<Ident>,
        extends: Option<TypeAnnotation>,
        implements: Vec<TypeAnnotation>,
        members: Vec<ClassMember>,
        doc: Option<DocComment>,
    },
    /// Enum declaration. No const enums, no computed members. `value: None` means auto-numbered at typecheck time.
    EnumDecl {
        name: Ident,
        members: Vec<EnumMember>,
        doc: Option<DocComment>,
    },
    TypeAliasDecl {
        name: Ident,
        generics: Vec<Ident>,
        ty: TypeAnnotation,
        doc: Option<DocComment>,
    },
    /// Top-level only; nested imports are a parse error.
    Import {
        module: String,
        module_span: Span,
        kind: ImportKind,
        doc: Option<DocComment>,
    },
    /// Re-export (`export { a, b as c } from "./util";` or `export { x };`).
    /// Top-level only. Parses in single-file but the typechecker gates it
    /// until cross-module packages land. `source` is the `from "…"` module
    /// path + span, or `None` for the bare `export { x };` form.
    ExportFrom {
        specs: Vec<ImportSpecifier>,
        source: Option<(String, Span)>,
        doc: Option<DocComment>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum ImportKind {
    Named(Vec<ImportSpecifier>),
    /// Compile-time namespace — namespace value is not first-class (`const x = uuid;` is rejected).
    Namespace {
        local_name: Ident,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImportSpecifier {
    pub imported_name: Ident,
    pub local_name: Ident,
}

#[derive(Clone, Debug, PartialEq)]
pub enum InterfaceMember {
    IndexSignature(IndexSignatureAnnotation),
    Method {
        name: Ident,
        optional: bool,
        generics: Vec<Ident>,
        params: Vec<ParamDecl>,
        return_type: TypeAnnotation,
        span: Span,
        doc: Option<DocComment>,
    },
    /// Property. `readonly` forbids writes through the interface; `optional: true`
    /// widens reads to `T | undefined`.
    Property {
        name: Ident,
        ty: TypeAnnotation,
        optional: bool,
        readonly: bool,
        span: Span,
        doc: Option<DocComment>,
    },
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

/// Modifiers on a class field or method. `readonly`/`visibility_span` carry spans so
/// later phases can anchor diagnostics (e.g. assignment to a `readonly` field).
#[derive(Clone, Debug, PartialEq)]
pub struct ClassModifiers {
    pub visibility: Visibility,
    pub visibility_span: Option<Span>,
    pub readonly: Option<Span>,
    pub static_span: Option<Span>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ClassMember {
    Field {
        name: Ident,
        modifiers: ClassModifiers,
        /// `x?: T`.
        optional: bool,
        ty: TypeAnnotation,
        /// `= expr`. Semantics owned by the typechecker.
        initializer: Option<ExprId>,
        span: Span,
        doc: Option<DocComment>,
    },
    Method {
        name: Ident,
        optional: bool,
        modifiers: ClassModifiers,
        generics: Vec<Ident>,
        params: Vec<ParamDecl>,
        return_type: TypeAnnotation,
        body: StmtId,
        span: Span,
        doc: Option<DocComment>,
    },
    Constructor {
        /// `private constructor`: only the class's own module may call it or
        /// extend the class.
        visibility: Visibility,
        params: Vec<ParamDecl>,
        body: StmtId,
        span: Span,
        doc: Option<DocComment>,
    },
    /// `get x(): T { … }` or `set x(v: T) { … }`. Get/set of the same name pair
    /// into one property; the typechecker validates and pairs them.
    Accessor {
        name: Ident,
        modifiers: ClassModifiers,
        kind: AccessorKind,
        /// The setter's single parameter; `None` for a getter. Boxed to keep the
        /// variant near its siblings' size (clippy `large_enum_variant`).
        param: Option<Box<ParamDecl>>,
        /// The getter's return type; `None` for a setter.
        return_type: Option<TypeAnnotation>,
        body: StmtId,
        span: Span,
        doc: Option<DocComment>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AccessorKind {
    Get,
    Set,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EnumMember {
    pub name: Ident,
    pub value: Option<EnumInitializer>,
    pub span: Span,
    pub doc: Option<DocComment>,
}

/// Enum member initializer. Restricted to numeric and string literals (numeric may be negated).
#[derive(Clone, Debug, PartialEq)]
pub enum EnumInitializer {
    /// `= 1` or `= -1` — `span` covers the leading minus when present.
    Number {
        value: f64,
        span: Span,
    },
    String {
        value: String,
        span: Span,
    },
}

impl EnumInitializer {
    pub fn span(&self) -> Span {
        match self {
            EnumInitializer::Number { span, .. } => *span,
            EnumInitializer::String { span, .. } => *span,
        }
    }
}

/// `values` has multiple entries when consecutive `case` labels share a body.
#[derive(Clone, Debug, PartialEq)]
pub struct SwitchCase {
    pub values: Vec<ExprId>,
    pub body: StmtId,
    pub span: Span,
}

/// Separate from `SwitchCase` so duplicate-default detection has nowhere to slip.
#[derive(Clone, Debug, PartialEq)]
pub struct SwitchDefault {
    pub body: StmtId,
    pub span: Span,
}

/// `catch (e)` and bindingless `catch` are catch-all clauses.
/// A bindingless clause uses an inaccessible compiler identifier.
/// When present, the typechecker verifies `ty` is an Error class.
#[derive(Clone, Debug, PartialEq)]
pub struct CatchClause {
    pub binding: Ident,
    pub ty: Option<TypeAnnotation>,
    pub body: StmtId,
    pub span: Span,
}

/// Source-syntax parameter. Fully-resolved form is [`crate::Param`].
#[derive(Clone, Debug, PartialEq)]
pub struct ParamDecl {
    pub name: Ident,
    pub ty: Option<TypeAnnotation>,
    pub default: Option<ExprId>,
    /// Declared with `?`. A parameter with a default may be omitted too;
    /// [`ParamDecl::is_omittable`] asks that.
    pub optional: bool,
    /// Cleared by `lower_patterns`; always `None` post-lowering.
    pub pattern: Option<Binding>,
    pub rest: bool,
    /// `Some` for a constructor parameter property (`constructor(public x: T)`):
    /// declares and auto-assigns a field. `None` for an ordinary parameter.
    pub modifiers: Option<ClassModifiers>,
}

impl ParamDecl {
    /// Whether a caller may leave the argument out: declared `?`, or defaulted.
    pub fn is_omittable(&self) -> bool {
        self.optional || self.default.is_some()
    }
}

/// Destructuring binding pattern. Plain `Ident` bindings stay as `Let`/`Const` stmts; only `{…}` / `[…]` forms appear here.
#[derive(Clone, Debug, PartialEq)]
pub enum Binding {
    Object {
        fields: Vec<ObjectPatternField>,
        rest: Option<Ident>,
        span: Span,
    },
    Array {
        /// `None` entries are holes (`[, x]`).
        elems: Vec<Option<Ident>>,
        /// Defaults at the corresponding element positions; holes have no default.
        defaults: Vec<Option<ExprId>>,
        rest: Option<Ident>,
        span: Span,
    },
}

impl Binding {
    pub fn span(&self) -> Span {
        match self {
            Binding::Object { span, .. } | Binding::Array { span, .. } => *span,
        }
    }
}

/// `source` is the field name on the RHS; `local` is the binding name. Equal for shorthand `{ a }`, differ for `{ a: x }`.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectPatternField {
    pub source: Ident,
    pub local: Ident,
    pub default: Option<ExprId>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypeAnnotation {
    pub kind: TypeAnnotationKind,
    pub span: Span,
}

/// Surface form of `<param> is <T>`. Only valid at a function return-type position.
#[derive(Clone, Debug, PartialEq)]
pub struct TypePredicateAnnotation {
    pub param: Ident,
    pub asserted: TypeAnnotation,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TypeAnnotationKind {
    /// `name.span` covers the identifier only; outer span includes any `<…>` args.
    Name {
        name: Ident,
        args: Vec<TypeAnnotation>,
    },
    /// Dotted type like `Temporal.Instant`. `path.len() >= 2`; last segment is the type name.
    Qualified {
        path: Vec<Ident>,
        args: Vec<TypeAnnotation>,
    },
    StringLiteral(String),
    /// Parser canonicalizes `-0.0` to `0.0`.
    NumberLiteral(crate::types::LiteralF64),
    /// Decimal digits with a leading `-` when negative, as in [`crate::Type::BigIntLiteral`].
    BigIntLiteral(String),
    BooleanLiteral(bool),
    Array(Box<TypeAnnotation>),
    /// Element labels (`[x: number, y: number]`) are documentation only, so the
    /// parser checks and drops them.
    Tuple(Vec<TypeAnnotation>),
    /// An omittable tuple element (`T?` or `name?: T`).
    Optional(Box<TypeAnnotation>),
    /// `readonly T[]` / `readonly [A, B]`. The parser only builds this around an
    /// [`Array`](Self::Array) or [`Tuple`](Self::Tuple) operand.
    Readonly(Box<TypeAnnotation>),
    Object {
        fields: Vec<TypeAnnotationField>,
        index: Option<Box<IndexSignatureAnnotation>>,
    },
    /// Function type annotation. Only the named-parameter form is accepted.
    Function {
        params: Vec<TypeAnnotationField>,
        return_type: Box<TypeAnnotation>,
    },
    /// Always ≥2 members; single-element unions are unwrapped by the parser.
    Union(Vec<TypeAnnotation>),
    /// `keyof T` — the union of `T`'s member names as string literal types.
    /// Resolved eagerly, so this never reaches the typed AST.
    KeyOf(Box<TypeAnnotation>),
    /// `typeof x` — the type of the *value* `x`, looked up in the value namespace
    /// rather than the type namespace. `path` is the dotted reference, one span per
    /// segment (`typeof o.k` has two). Resolved eagerly, like [`KeyOf`](Self::KeyOf).
    TypeOf {
        path: Vec<Ident>,
    },
}

/// Reused for object-type fields and function-type parameters. `rest` is always
/// `false` in object position; `readonly` is only meaningful for object-type fields
/// (always `false` for function/tuple parameters).
#[derive(Clone, Debug, PartialEq)]
pub struct TypeAnnotationField {
    pub name: Ident,
    pub ty: TypeAnnotation,
    pub optional: bool,
    pub readonly: bool,
    pub rest: bool,
    /// Declared with method syntax, `m(): T`, rather than as a property `m: () => T`.
    pub method: bool,
}

/// A destructuring default, lowered to `saved === undefined ? default : saved`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindingDefault {
    /// The component read, saved once before the default is chosen.
    pub saved: Ident,
    /// The default expression.
    pub default: ExprId,
    /// Whether the pattern's root has a type annotation, which the default
    /// must then fit.
    pub annotated: bool,
}

/// Metadata for a synthesized `IndexAccess` from pattern lowering — lets the typechecker emit destructure-specific errors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatternOrigin {
    pub pattern_span: Span,
    pub slot_arity: usize,
}

/// A top-level declaration marked `export` (Form 1: `export function …`,
/// `export const …`, etc.). The declaration itself stays a plain node in
/// `top_level`; this side record only tracks which ones are export-marked, so
/// the inference passes need no `export` awareness.
#[derive(Clone, Debug)]
pub struct ExportedDecl {
    pub stmt: StmtId,
    /// Span of the `export` keyword — used for diagnostics and `ExportEntry`.
    pub export_span: Span,
}

#[derive(Default, Clone, Debug)]
pub struct Ast {
    exprs: Vec<Expr>,
    stmts: Vec<Stmt>,
    pub top_level: Vec<StmtId>,
    /// Maps synthesized `IndexAccess` IDs → their source pattern (populated by `lower_patterns`).
    /// `BTreeMap` is precautionary: the compiler only ever looks entries up by `ExprId`, and
    /// the parser snapshots that render `Ast`'s `Debug` all run before `lower_patterns`
    /// populates this, so order is unobserved today. An ordered map keeps any future walk —
    /// or post-lowering snapshot — independent of the hash seed.
    pub pattern_origins: std::collections::BTreeMap<ExprId, PatternOrigin>,
    /// Array literals that an array pattern destructures directly, as in
    /// `let [a, b] = [1, "s"]`. TypeScript types such a literal as a tuple from
    /// the pattern, so each name gets its element's type and a pattern longer
    /// than the literal is a compile error (populated by `lower_patterns`).
    pub tuple_pattern_sources: std::collections::BTreeSet<ExprId>,
    /// Pattern decomposition statements, run immediately after their parameter initializes.
    pub parameter_bindings: std::collections::HashMap<Span, Vec<StmtId>>,
    /// Source names in lowered for-of heads, whose TDZ includes the iterable.
    pub for_of_pattern_bindings: std::collections::BTreeMap<StmtId, Vec<Ident>>,
    /// The `let` names a destructuring `for` initializer declared, by the
    /// lowered `for`, whose initializer now precedes it (populated by
    /// `lower_patterns`).
    pub for_pattern_init_bindings: std::collections::BTreeMap<StmtId, Vec<Ident>>,
    /// Top-level declarations carrying a leading `export` (Form 1).
    pub exported_decls: Vec<ExportedDecl>,
    /// The `undefined` a typed `let x: T;` starts with, synthesized by the parser.
    pub implicit_initializers: std::collections::BTreeSet<ExprId>,
    /// Every `undefined` the compiler writes itself: implicit initializers and
    /// the padding of a destructured tuple. Each reads the value `undefined`,
    /// even where a binding named `undefined` is in scope.
    pub synthetic_undefined: std::collections::BTreeSet<ExprId>,
    /// Destructuring defaults, keyed by the ternary `lower_patterns` makes of each.
    pub binding_defaults: std::collections::BTreeMap<ExprId, BindingDefault>,
}

impl Ast {
    pub(crate) fn source_expressions(&self) -> &[Expr] {
        &self.exprs
    }
    pub(crate) fn source_statements(&self) -> &[Stmt] {
        &self.stmts
    }

    pub fn validate_source(
        &self,
        source: &str,
        file: crate::FileId,
    ) -> Result<(), crate::source::SourceError> {
        crate::source_validation::validate(self, source, file)
    }

    pub fn new() -> Self {
        Self::default()
    }

    /// Checked allocation; failure leaves the arena unchanged.
    pub fn try_push_expr(&mut self, expr: Expr) -> Result<ExprId, ArenaError> {
        arena::push(&mut self.exprs, expr, ArenaKind::Expressions).map(ExprId)
    }

    /// Checked allocation; failure leaves the arena unchanged.
    pub fn try_push_stmt(&mut self, stmt: Stmt) -> Result<StmtId, ArenaError> {
        arena::push(&mut self.stmts, stmt, ArenaKind::Statements).map(StmtId)
    }

    pub fn try_expr(&self, id: ExprId) -> Result<&Expr, ArenaError> {
        arena::get(&self.exprs, id.0, ArenaKind::Expressions)
    }

    pub fn try_stmt(&self, id: StmtId) -> Result<&Stmt, ArenaError> {
        arena::get(&self.stmts, id.0, ArenaKind::Statements)
    }

    pub fn try_expr_mut(&mut self, id: ExprId) -> Result<&mut Expr, ArenaError> {
        arena::get_mut(&mut self.exprs, id.0, ArenaKind::Expressions)
    }

    pub fn try_stmt_mut(&mut self, id: StmtId) -> Result<&mut Stmt, ArenaError> {
        arena::get_mut(&mut self.stmts, id.0, ArenaKind::Statements)
    }

    /// Snapshot of allocated expression IDs, usable while appending new nodes.
    pub fn expr_ids(&self) -> Result<impl DoubleEndedIterator<Item = ExprId> + use<>, ArenaError> {
        Ok(arena::ids(self.exprs.len(), ArenaKind::Expressions)?.map(ExprId))
    }

    /// Snapshot of allocated statement IDs, usable while appending new nodes.
    pub fn stmt_ids(&self) -> Result<impl DoubleEndedIterator<Item = StmtId> + use<>, ArenaError> {
        Ok(arena::ids(self.stmts.len(), ArenaKind::Statements)?.map(StmtId))
    }

    pub fn exprs_len(&self) -> usize {
        self.exprs.len()
    }

    pub fn stmts_len(&self) -> usize {
        self.stmts.len()
    }
}

#[cfg(test)]
mod tests {
    use super::{Ast, BinOp, Expr, ExprKind, Stmt, StmtKind, UnOp};
    use crate::Span;

    #[test]
    fn arena_round_trip_for_exprs() {
        let mut ast = Ast::new();
        let id = ast
            .try_push_expr(Expr {
                kind: ExprKind::Number(42.0),
                span: Span::new(crate::FileId(0), 0, 2).unwrap(),
            })
            .unwrap();
        assert_eq!(id.0, 0);
        assert_eq!(ast.try_expr(id).unwrap().kind, ExprKind::Number(42.0));
        assert_eq!(
            ast.try_expr(id).unwrap().span,
            Span::new(crate::FileId(0), 0, 2).unwrap()
        );
    }

    #[test]
    fn arena_round_trip_for_stmts() {
        let mut ast = Ast::new();
        let expr_id = ast
            .try_push_expr(Expr {
                kind: ExprKind::Null,
                span: Span::new(crate::FileId(0), 0, 4).unwrap(),
            })
            .unwrap();
        let stmt_id = ast
            .try_push_stmt(Stmt {
                kind: StmtKind::Expr(expr_id),
                span: Span::new(crate::FileId(0), 0, 5).unwrap(),
            })
            .unwrap();
        assert_eq!(stmt_id.0, 0);
        assert_eq!(
            ast.try_stmt(stmt_id).unwrap().span,
            Span::new(crate::FileId(0), 0, 5).unwrap()
        );
        assert!(matches!(
            ast.try_stmt(stmt_id).unwrap().kind,
            StmtKind::Expr(_)
        ));
    }

    #[test]
    fn build_binary_expression_tree() {
        let mut ast = Ast::new();
        let lhs = ast
            .try_push_expr(Expr {
                kind: ExprKind::Number(1.0),
                span: Span::new(crate::FileId(0), 0, 1).unwrap(),
            })
            .unwrap();
        let rhs = ast
            .try_push_expr(Expr {
                kind: ExprKind::Number(2.0),
                span: Span::new(crate::FileId(0), 4, 5).unwrap(),
            })
            .unwrap();
        let sum = ast
            .try_push_expr(Expr {
                kind: ExprKind::Binary {
                    op: BinOp::Add,
                    lhs,
                    rhs,
                },
                span: Span::new(crate::FileId(0), 0, 5).unwrap(),
            })
            .unwrap();
        let stmt_id = ast
            .try_push_stmt(Stmt {
                kind: StmtKind::Expr(sum),
                span: Span::new(crate::FileId(0), 0, 6).unwrap(),
            })
            .unwrap();
        ast.top_level.push(stmt_id);

        let top = ast.try_stmt(stmt_id).unwrap();
        let StmtKind::Expr(sum_id) = top.kind else {
            panic!("expected expression statement")
        };
        let sum_expr = ast.try_expr(sum_id).unwrap();
        let ExprKind::Binary { op, lhs, rhs } = sum_expr.kind else {
            panic!("expected binary expression")
        };
        assert_eq!(op, BinOp::Add);
        assert_eq!(ast.try_expr(lhs).unwrap().kind, ExprKind::Number(1.0));
        assert_eq!(ast.try_expr(rhs).unwrap().kind, ExprKind::Number(2.0));
    }

    #[test]
    fn clone_and_equality() {
        let kind = ExprKind::Unary {
            op: UnOp::Neg,
            operand: super::ExprId(0),
        };
        assert_eq!(kind.clone(), kind);
        assert_eq!(BinOp::Eq, BinOp::Eq);
        assert_ne!(BinOp::Eq, BinOp::NotEq);
    }

    #[test]
    fn new_ast_is_empty() {
        let ast = Ast::new();
        assert!(ast.top_level.is_empty());
    }
}

/// A string index signature; the parameter name is documentation only.
#[derive(Clone, Debug, PartialEq)]
pub struct IndexSignatureAnnotation {
    pub value: TypeAnnotation,
    pub readonly: bool,
    pub span: Span,
}
