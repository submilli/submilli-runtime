//! Which of the caller's values reach a `check()` in one body, when the body
//! is simple enough to tell; otherwise, that every one may.
//!
//! # What reaches `check()`
//!
//! A value reaches `check()` as data when it flows into an argument of a
//! `check()`, and as control when it decides whether or which `check()`
//! runs. The walk follows values through locals (a local holds everything
//! ever assigned to it), operators, literals, conditionals and calls (a
//! call's result carries its receiver and its arguments). A condition
//! decides when its construct holds a `check()`, or when a branch leaves
//! (`return`, `throw`, `break` out of a loop, `continue`) before a `check()`
//! or inside a loop that holds one. A local assigned in a branch, or after a
//! branch that left, holds what decided the branch as well, so a flag set
//! under a condition carries the condition.
//!
//! A value only compared with `null`, tested for truthiness, given to
//! `typeof` or `instanceof`, or compared with another `Map`, `Set` or
//! regular expression reaches `check()` by its identity: only reading it
//! again, or handing on a value that holds it, can change the decision. Any
//! other comparison compares arrays, objects and class instances by their
//! stored contents, so it uses the whole value, also when the value is a
//! caller's parameter itself.
//!
//! # Simple and complex bodies
//!
//! The walk sees values, not where the objects they name are kept. It stays
//! exact only while nothing that reaches `check()` can change behind its
//! back. A body is complex when:
//!
//! 1. what reaches `check()` is read from shared state, which code outside
//!    the body can change: `this`, a module `let`, a writable static field, a
//!    global of another package, or a name with no binding. A module `const`
//!    or a `readonly` static field cannot be rebound, so it is not shared
//!    state.
//! 2. a container the body built (an array, object, `Map` or `Set` in a
//!    local, made new, or a call's result made from nothing the caller
//!    supplied) reaches `check()` and is shared. A local made from a value
//!    that is not new and carries the caller's data (`input.lists.find(..)`,
//!    `map.get(..)`, a helper's result, `copy.pop()`) may be the caller's
//!    object, so it is not the body's own, and a write into it counts under
//!    4. Sharing is:
//!    - another binding is made from it, a nested function captures it, or
//!      it is passed to a call that also receives a caller's object, which
//!      the callee could store into it or read again;
//!    - a direct write into it, or a call that passes it with only
//!      primitives, module constants and other such containers, is followed
//!      exactly; an object or array literal argument counts member by
//!      member;
//!    - a binding made from it when it is never used again (no read, write,
//!      call, capture or further binding after that point, nor earlier in a
//!      loop around that point that its declaration is not in) is a move,
//!      not sharing: the new binding holds the container from then on, and
//!      the body is walked again knowing it. So in
//!      `const copied: string[] = []; for (const e of requested) copied.push(e); cc = copied;`
//!      `cc` holds the body's own container.
//! 3. a nested function assigns a local that reaches `check()`, or the body
//!    calls a function value (a nested function, or one a local or a global
//!    holds, also as `f?.(x)`) whose result reaches `check()` or which is
//!    passed a local that does;
//! 4. the body changes a value it does not own (see [`Walker::is_foreign`]):
//!    it writes a field or an element of it, or calls, with arguments, a
//!    method that may change it. A method with no arguments (`pop`,
//!    `clear`, `reverse`) cannot write a value into it, so it is exempt. For
//!    an array only `push`, `pop`, `shift`, `unshift`, `splice`, `sort`,
//!    `reverse`, `fill` and `copyWithin` may change it; for a `Map` or `Set`
//!    only `set`, `add`, `delete` and `clear`; for a `Uint8Array` only `set`,
//!    `fill`, `copyWithin`, `sort` and `reverse`; a string's, a regular
//!    expression's, a Temporal value's, a `TextEncoder`'s or a
//!    `TextDecoder`'s methods never do; any other method may. A new value
//!    (a literal, `new` of a builtin such as `new Map()` or
//!    `new Uint8Array(n)`, `Array.from`, `Object.keys`, `JSON.parse`, or a
//!    copy such as `slice`, `map` or `split`) is the body's own; `new` of a
//!    package class is a call of its constructor, whose result is not new.
//!    The change may be read back where the walk cannot see it. Assigning a
//!    module `let` or a static field counts too;
//! 5. the body calls itself, so a write after its `check()` runs before the
//!    `check()` of the next call;
//! 6. following it takes more than [`WORK_BUDGET`] steps.
//!
//! In a complex body every caller-supplied value is examined, as if none
//! were known not to reach `check()`, and each warning says why.
//!
//! A write or call after the last `check()` of the body, in no loop that
//! holds a `check()` and not inside a nested function, is left out of 2, 3
//! and 4: every `check()` has already run. A `check()` inside a nested
//! function may run at any time, so then nothing is left out. 5 and 6 always
//! apply.
//!
//! # Not followed
//!
//! - Package state one exported function writes and another checks, or
//!   that a helper keeps: `remember(input.id)` into a module array, read
//!   back by `pendingIds()` before the `check()`.
//! - A `check()` in one exported function and the use in a function that
//!   calls it.
//! - A helper that decides whether `check()` runs by throwing, as
//!   `requireOwn(input.id)` before it, or
//!   `try { requireOwn(input.id) } catch { return; }`.
//! - Calls through an interface, whose callee is not known.
//! - A comparison of a whole caller array or object that decides a
//!   `check()`, followed by a read of its elements: the comparison is not
//!   counted as a read of each element.
//!
//! The skill and its security review cover these.
//!
//! # Naming
//!
//! [`flow`](super::flow) names the values it reports by the same
//! [`Origin`]s: a parameter by its [`ParameterId`], `this` only in a body with
//! a receiver, a name with no binding as [`ValueRoot::Unresolved`], a global
//! by its mangled name, and each read by [`PackageFacts::index_key`] or
//! [`ReadKey::of_path`]. A value the two walks named differently would never
//! be found to reach `check()`.

use std::collections::{BTreeMap, BTreeSet};

use super::super::check_calls::{CheckCall, SearchRoot};
use super::bodies::{Body, is_module_const, is_static_field};
use super::origin::{Origin, ParameterId, ReadKey, ValueRoot};
use super::package::{PackageFacts, is_runtime_symbol, parameter_shown, source_name};
use super::scope::Scopes;
use super::stability::is_primitive;
use crate::compiler_error::CompilerFailure;
use crate::typechecker::infer::narrowing::{BindingId, ReferencePath};
use crate::{
    BinOp, ClosureBody, ExprId, Ident, MangledName, PostfixTarget, Span, StmtId, Type,
    TypedCatchClause, TypedChainPart, TypedExpr, TypedExprKind, TypedParam, TypedStmtKind, UnOp,
};

/// The most steps one body may take: each value a local is made with (its
/// declaration, a for-of element, a function value's result, a condition's
/// values), each value written into one later (with a reference to each
/// condition it is written under), each step of finding a local's values
/// made from the caller's, and each value resolved through a local, counted
/// once per read in its path. The busiest body of the first-party packages
/// took under 7,000 when the limit was last measured, well under 1% of it;
/// a body that needs more is generated, or so tangled that examining its
/// every value costs little precision.
const WORK_BUDGET: usize = 1_000_000;

/// The array methods that may change the array they are called on.
const ARRAY_MUTATORS: &[&str] = &[
    "push",
    "pop",
    "shift",
    "unshift",
    "splice",
    "sort",
    "reverse",
    "fill",
    "copyWithin",
];

/// The `Map` and `Set` methods that may change the collection.
const COLLECTION_MUTATORS: &[&str] = &["set", "add", "delete", "clear"];

/// The `Uint8Array` methods that may change the bytes.
const BYTES_MUTATORS: &[&str] = &["set", "fill", "copyWithin", "sort", "reverse"];

/// The static methods of a builtin constructor that return a new value.
const CONSTRUCTORS: &[&str] = &[
    "new",
    "from",
    "of",
    "keys",
    "values",
    "entries",
    "fromEntries",
];

/// The array methods that return a new array.
const ARRAY_COPIES: &[&str] = &[
    "slice",
    "map",
    "filter",
    "concat",
    "flat",
    "flatMap",
    "toSorted",
    "toReversed",
    "toSpliced",
    "with",
];

/// The string methods that return a new array.
const STRING_COPIES: &[&str] = &["split", "match"];

// The result: what reaches `check()`, or why every value is examined.

/// The caller's values that reach a `check()`, with where each first does,
/// or why every one is examined.
pub(super) struct Checked {
    reaches: Vec<Reach>,
    complex: Option<Complexity>,
}

/// Why every caller-supplied value of a body is examined.
struct Complexity {
    span: Span,
    reason: String,
}

/// Where a caller's value reaches `check()`, and what of it `check()` relies on.
struct Reach {
    origin: Origin,
    reliance: Reliance,
    /// The value as its root's name and the reads from it show it.
    shown: String,
    span: Span,
    kind: ReachKind,
}

/// How a value reaches `check()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReachKind {
    /// It flows into an argument of `check()`.
    Data,
    /// It decides whether a `check()` runs.
    Control,
}

/// What of a value `check()`, or a computation, relies on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Reliance {
    /// Its value, and so everything it holds.
    Value,
    /// Only its identity: whether it is `null`, truthy, of a type, or the
    /// same `Map`, `Set` or regular expression as another.
    Identity,
}

/// What a report does with a caller's value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Access {
    /// Reads it from the value that holds it.
    Read,
    /// Hands it on, or uses what it holds.
    Pass,
}

impl ReachKind {
    /// The note that `shown` reaches `check()` this way.
    pub(super) fn note(self, shown: &str) -> String {
        match self {
            ReachKind::Data => format!("`{shown}` reaches `check()` here"),
            ReachKind::Control => format!("`{shown}` decides whether `check()` runs here"),
        }
    }
}

impl Checked {
    /// Where `access` to the value at `origin` can change what reaches
    /// `check()`, as the note that says so: the earliest place, or, in a
    /// complex body, why every value is examined.
    pub(super) fn reach(&self, origin: &Origin, access: Access) -> Option<(Span, String)> {
        if let Some(complex) = &self.complex {
            return Some((complex.span, complex.reason.clone()));
        }
        self.reaches
            .iter()
            .filter(|reach| reach.is_changed_by(origin, access))
            .min_by_key(|reach| position(reach.span))
            .map(|reach| (reach.span, reach.kind.note(&reach.shown)))
    }
}

impl Reach {
    /// A value `check()` relies on changes with the value at `origin` when
    /// one may hold, or count the elements of, the other. An identity
    /// changes only with a read of the value, or of one that holds it, or by
    /// handing on one that holds it.
    fn is_changed_by(&self, origin: &Origin, access: Access) -> bool {
        if !self.origin.is_related(origin) {
            return false;
        }
        let (depth, reached) = (origin.steps.len(), self.origin.steps.len());
        match (self.reliance, access) {
            (Reliance::Value, _) => true,
            (Reliance::Identity, Access::Read) => depth <= reached,
            (Reliance::Identity, Access::Pass) => depth < reached,
        }
    }
}

pub(super) fn analyse(
    package: &PackageFacts<'_>,
    body: &Body<'_>,
    checks: &[CheckCall],
    checking_closures: &BTreeSet<ExprId>,
) -> Result<Checked, CompilerFailure> {
    let mut first = Walker::new(package, body, checks, checking_closures, None);
    first.walk_body()?;
    let moves = first.moves();
    if moves.is_empty() {
        return first.finish();
    }
    // Which locals hold a container the body built is known only once every
    // move is, so the body is walked again knowing them.
    let ownership = first.ownership(moves);
    let mut second = Walker::new(package, body, checks, checking_closures, Some(ownership));
    second.walk_body()?;
    second.finish()
}

// What the walk records.

/// A binding of the body, by the order the walk declares it in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct LocalId(usize);

/// Where a value comes from: a local of the body, or a value the body did
/// not bind, which `Caller` names by its root: a parameter, `this`, any
/// global, or a name with no binding.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Source {
    Caller(ValueRoot),
    Local(LocalId),
}

/// A value an expression may yield, or one the value is computed from.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Term {
    source: Source,
    steps: Vec<ReadKey>,
    /// Whether the value is computed from the one at `steps` rather than
    /// read from it. A read of a computed value reads no further along the
    /// path, so the term stands for everything beneath `steps`.
    derived: bool,
    /// What the computation used of the value it was computed from.
    used: Reliance,
}

impl Term {
    fn of(source: Source) -> Self {
        Self {
            source,
            steps: Vec::new(),
            derived: false,
            used: Reliance::Value,
        }
    }

    fn read(&self, key: &ReadKey) -> Self {
        let mut read = self.clone();
        if !read.derived {
            read.steps.push(key.clone());
        }
        read
    }
}

/// The values an expression may yield or be computed from.
type Flow = BTreeSet<Term>;

/// A local of the body.
struct Local {
    name: String,
    /// Everything assigned to it.
    flow: Flow,
    /// What it was declared or assigned as, before any computation: the
    /// values it may be, or be part of.
    made_from: BTreeSet<Source>,
    /// Whether it may itself be an object the body does not own: it was
    /// made from a value that is not new and carries the caller's data, or
    /// a global's. Any write into it may change that object.
    may_be_foreign: bool,
    /// Whether an object the body does not own was put inside it, which a
    /// write through one of its fields may then change.
    holds_foreign: bool,
    primitive: bool,
    /// How many nested functions enclose its declaration.
    depth: u32,
    /// The loops its declaration is in, by span.
    loops: Vec<Span>,
    /// Where it is read, written or passed after its declaration.
    uses: Vec<LocalUse>,
}

/// A reference to a local.
struct LocalUse {
    span: Span,
    /// Whether a nested function makes it, which may run at any time.
    captured: bool,
}

impl Local {
    /// Whether it was made as a new container: not a primitive, not the
    /// caller's value or another local's under a new name, and not a value
    /// that may be an object the body does not own. A call's result made
    /// from nothing the caller supplied counts: it is computed, so
    /// `made_from` is empty.
    fn built_container(&self) -> bool {
        !self.primitive && self.made_from.is_empty() && !self.may_be_foreign
    }
}

/// A caller's value a local may hold.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Held {
    origin: Origin,
    /// As [`Term::derived`].
    derived: bool,
    /// What the local's value relies on of the value at `origin`.
    reliance: Reliance,
}

impl Held {
    fn of(origin: Origin, term: &Term) -> Self {
        Held {
            reliance: term.used,
            origin,
            derived: term.derived,
        }
    }

    /// What a local that holds `self` yields to a use of it as `term`.
    fn through(&self, term: &Term) -> Self {
        if self.derived {
            return self.clone();
        }
        Held::of(self.origin.after(&term.steps), term)
    }
}

/// Something the body does that may make it complex, depending on what
/// reaches `check()`.
enum Event {
    /// Another local is made from `from`. It is a move, not sharing, when
    /// `movable` and `from` is never used again.
    Alias {
        from: LocalId,
        into: LocalId,
        span: Span,
        movable: bool,
        /// The loops the alias is made in, by span.
        loops: Vec<Span>,
    },
    /// A nested function refers to a local declared outside it.
    Captured { local: LocalId, span: Span },
    /// A nested function assigns a local declared outside it.
    NestedAssign { local: LocalId, span: Span },
    /// Containers the body built are passed to a call with `others`.
    PassedWith {
        owned: Vec<LocalId>,
        others: Vec<Flow>,
        callee: String,
        span: Span,
    },
    /// A function value is called; its result is held by `result`.
    FunctionValueCall {
        result: LocalId,
        args: Flow,
        callee: String,
        span: Span,
    },
    /// A value the body does not own is changed.
    ForeignWrite { what: String, span: Span },
    /// The body calls itself.
    Recursion { span: Span },
}

/// How far the walk had come when a construct's branches began.
#[derive(Clone, Copy)]
struct Mark {
    checks_met: usize,
    exits: usize,
    returns: usize,
}

/// Values that reach `check()` at `span`.
struct Sink {
    flow: Flow,
    span: Span,
    kind: ReachKind,
}

/// Where each value, by what `check()` relies on of it, first reaches
/// `check()`, and how.
type Earliest = BTreeMap<(Origin, Reliance), (Span, ReachKind)>;

/// The steps taken so far against [`WORK_BUDGET`].
#[derive(Default)]
struct Work {
    spent: usize,
    exceeded: bool,
}

impl Work {
    /// Spends `steps`; false once the budget is gone.
    fn spend(&mut self, steps: usize) -> bool {
        self.spent = self.spent.saturating_add(steps);
        if self.spent > WORK_BUDGET {
            self.exceeded = true;
        }
        !self.exceeded
    }
}

/// What the walk tracks for the function it is in, which a nested function
/// starts afresh.
#[derive(Default)]
struct FunctionState {
    /// What the function returns.
    returned: Flow,
    /// How many `return`, `throw`, loop `break` and `continue` statements
    /// the walk has met, which tells whether a branch leaves.
    exits: usize,
    /// How many `return` and `throw` statements, which leave the function.
    returns: usize,
    /// The loops the walk is in, by their span.
    loops: Vec<Span>,
    /// The constructs a `break` may leave, innermost last.
    breakables: Vec<Breakable>,
    /// What decides each branch before this point that left: whether the
    /// code after it runs depends on it. Each is a condition's local.
    left_guards: Vec<LocalId>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Breakable {
    Loop,
    /// A `break` out of a `switch` continues after it, which leaves nothing.
    Switch,
}

/// A method's receiver, when the method may change it.
struct Changed<'f> {
    flow: &'f Flow,
    /// Whether the receiver is a new value (see [`is_fresh`]).
    fresh: bool,
}

struct Walker<'a, 'b> {
    package: &'b PackageFacts<'a>,
    body: &'b Body<'a>,
    checking_closures: &'b BTreeSet<ExprId>,
    /// Which locals hold a container the body built, once the moves of the
    /// body are known; `None` on the walk that finds them.
    ownership: Option<Ownership>,
    /// The name a member body calls itself by on `this`.
    member_name: Option<String>,
    /// Where each `check()` of the body is called.
    check_spans: Vec<Span>,
    /// Where the last `check()` of the body ends, when every one is in the
    /// body itself.
    last_check: Option<Span>,
    /// Every loop the walk is in, nested functions' included, by its span.
    enclosing_loops: Vec<Span>,
    /// How many nested functions enclose the walk.
    closure_depth: u32,
    function: FunctionState,
    scopes: Scopes<LocalId>,
    /// How messages name each caller-supplied parameter.
    parameters: BTreeMap<ParameterId, String>,
    locals: Vec<Local>,
    sinks: Vec<Sink>,
    events: Vec<Event>,
    work: Work,
    /// How many `check()` calls the walk has met, which tells whether a
    /// construct holds one.
    checks_met: usize,
    /// What decides each branch the walk is in, outermost first. Each is a
    /// condition's local, which a local assigned in the branch refers to.
    guards: Vec<LocalId>,
    /// A node two parents share is evaluated once at run time.
    evaluated: BTreeMap<ExprId, Flow>,
}

// Construction.
impl<'a, 'b> Walker<'a, 'b> {
    fn new(
        package: &'b PackageFacts<'a>,
        body: &'b Body<'a>,
        checks: &[CheckCall],
        checking_closures: &'b BTreeSet<ExprId>,
        ownership: Option<Ownership>,
    ) -> Self {
        Walker {
            package,
            body,
            checking_closures,
            ownership,
            member_name: member_name(body),
            check_spans: checks.iter().map(|call| call.span).collect(),
            last_check: last_check(checks),
            enclosing_loops: Vec::new(),
            closure_depth: 0,
            function: FunctionState::default(),
            scopes: Scopes::default(),
            parameters: BTreeMap::new(),
            locals: Vec::new(),
            sinks: Vec::new(),
            events: Vec::new(),
            work: Work::default(),
            checks_met: 0,
            guards: Vec::new(),
            evaluated: BTreeMap::new(),
        }
    }
}

/// The name a method or accessor calls itself by on `this`.
fn member_name(body: &Body<'_>) -> Option<String> {
    if !body.has_receiver {
        return None;
    }
    body.label
        .rsplit_once('.')
        .map(|(_, member)| member.to_string())
}

/// The last `check()`, when none is in a nested function, whose call may
/// run at any time.
fn last_check(checks: &[CheckCall]) -> Option<Span> {
    if checks.iter().any(|call| !call.closures.is_empty()) {
        return None;
    }
    checks
        .iter()
        .map(|call| call.span)
        .max_by_key(|span| position(*span))
}

// The walk: statements.
impl<'a> Walker<'a, '_> {
    fn walk_body(&mut self) -> Result<(), CompilerFailure> {
        let body = self.body;
        self.scopes.push();
        self.declare_params(body.params, None);
        let own_name = body
            .function_value
            .and_then(|function| self.package.ta.closure_names.get(&function));
        if let Some(name) = own_name {
            self.declare(name, Flow::new(), false);
        }
        let walked = match body.root {
            SearchRoot::Stmt(block) => self.stmt(block),
            SearchRoot::Expr(result) => self.eval(result).map(drop),
        };
        self.scopes.pop();
        walked
    }

    fn stmt(&mut self, id: StmtId) -> Result<(), CompilerFailure> {
        let ta = self.package.ta;
        let stmt = ta.try_stmt(id).map_err(crate::typechecker::arena_failure)?;
        let _: () = match &stmt.kind {
            TypedStmtKind::Let {
                name, ty, value, ..
            }
            | TypedStmtKind::Const {
                name, ty, value, ..
            } => {
                let flow = self.eval(*value)?;
                let sources = sources_read(&flow);
                let local = self.declare(name, flow, is_primitive(ty));
                self.note_made_from(local, *value)?;
                self.note_aliases(local, &sources, stmt.span, true);
                let guards = self.guard_terms();
                if self.work.spend(guards.len()) {
                    self.local_mut(local)?.flow.extend(guards);
                }
            }
            TypedStmtKind::Expr(value) => {
                self.eval(*value)?;
            }
            TypedStmtKind::AssignGlobal { mangled, value, .. } => {
                self.eval(*value)?;
                let global = self.package.global_shown(mangled);
                self.foreign_write(format!("the global `{global}`"), stmt.span);
            }
            TypedStmtKind::Throw { value } => {
                self.eval(*value)?;
                self.leave_function();
            }
            TypedStmtKind::Return(value) => {
                if let Some(value) = value {
                    let flow = self.eval(*value)?;
                    self.function.returned.extend(flow);
                }
                self.leave_function();
            }
            TypedStmtKind::Break => {
                if self.function.breakables.last() != Some(&Breakable::Switch) {
                    self.function.exits = self.function.exits.saturating_add(1);
                }
            }
            TypedStmtKind::Continue => {
                self.function.exits = self.function.exits.saturating_add(1);
            }
            TypedStmtKind::AssignLocal { ident, value, .. } => {
                let flow = self.eval(*value)?;
                if let Some(local) = self.scopes.resolve(&ident.name).copied() {
                    self.note_made_from(local, *value)?;
                    self.reassign(local, flow, stmt.span)?;
                }
            }
            TypedStmtKind::AssignField {
                receiver, value, ..
            } => {
                let receiver = self.eval(*receiver)?;
                let stores_foreign = self.passes_foreign_object(*value)?;
                let value = self.eval(*value)?;
                self.write_into(&receiver, value, stores_foreign, stmt.span)?;
            }
            TypedStmtKind::AssignIndex {
                receiver,
                index,
                value,
                ..
            } => {
                let receiver = self.eval(*receiver)?;
                let stores_foreign = self.passes_foreign_object(*value)?;
                let mut stored = self.eval(*index)?;
                stored.extend(self.eval(*value)?);
                self.write_into(&receiver, stored, stores_foreign, stmt.span)?;
            }
            TypedStmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                let decider = identity(self.eval(*condition)?);
                let since = self.mark();
                let guard = self.condition_local(&decider);
                self.guarded(guard, |walker| {
                    walker.scoped(*then_block)?;
                    match else_block {
                        Some(else_block) => walker.scoped(*else_block),
                        None => Ok(()),
                    }
                })?;
                self.branches_decide(since, decider, guard, *condition, stmt.span)?;
            }
            TypedStmtKind::While { condition, body }
            | TypedStmtKind::DoWhile { body, condition } => {
                let decider = identity(self.eval(*condition)?);
                let since = self.mark();
                let guard = self.condition_local(&decider);
                self.in_loop(stmt.span, guard, |walker| walker.scoped(*body))?;
                self.loop_decides(since, decider, guard, *condition, stmt.span)?;
            }
            TypedStmtKind::For {
                init,
                condition,
                update,
                body,
            } => {
                self.scopes.push();
                let walked = self.for_loop(*init, *condition, *update, *body, stmt.span);
                self.scopes.pop();
                walked?;
            }
            TypedStmtKind::ForOf {
                name,
                element_ty,
                iter,
                body,
                ..
            } => self.for_of(name, element_ty, *iter, *body, stmt.span)?,
            TypedStmtKind::Switch {
                discriminant,
                cases,
                default,
                ..
            } => {
                let decider = identity(self.eval(*discriminant)?);
                for comparison in cases
                    .iter()
                    .flat_map(crate::TypedSwitchCase::label_comparisons)
                {
                    self.eval(comparison)?;
                }
                let since = self.mark();
                let guard = self.condition_local(&decider);
                self.function.breakables.push(Breakable::Switch);
                let walked = self.guarded(guard, |walker| {
                    for case in cases {
                        walker.scoped(case.body)?;
                    }
                    match default {
                        Some(default) => walker.scoped(*default),
                        None => Ok(()),
                    }
                });
                self.function.breakables.pop();
                walked?;
                self.branches_decide(since, decider, guard, *discriminant, stmt.span)?;
            }
            TypedStmtKind::Try {
                body,
                catches,
                finally,
            } => {
                self.scoped(*body)?;
                for clause in catches {
                    self.catch(clause)?;
                }
                if let Some(finally) = finally {
                    self.scoped(*finally)?;
                }
            }
            TypedStmtKind::Block(stmts) => {
                self.scopes.push();
                let walked = stmts.iter().try_for_each(|stmt| self.stmt(*stmt));
                self.scopes.pop();
                walked?;
            }
            // `source` is what the region's references read again at each
            // use, and they carry its path.
            TypedStmtKind::NarrowRegion { body, .. } => self.stmt(*body)?,
            TypedStmtKind::ReboxLocal { .. } => {}
        };
        Ok(())
    }

    fn scoped(&mut self, id: StmtId) -> Result<(), CompilerFailure> {
        self.scopes.push();
        let walked = self.stmt(id);
        self.scopes.pop();
        walked
    }

    fn leave_function(&mut self) {
        self.function.exits = self.function.exits.saturating_add(1);
        self.function.returns = self.function.returns.saturating_add(1);
    }

    fn for_loop(
        &mut self,
        init: Option<StmtId>,
        condition: Option<ExprId>,
        update: Option<StmtId>,
        body: StmtId,
        span: Span,
    ) -> Result<(), CompilerFailure> {
        if let Some(init) = init {
            self.stmt(init)?;
        }
        let decider = match condition {
            Some(condition) => identity(self.eval(condition)?),
            None => Flow::new(),
        };
        let since = self.mark();
        let guard = self.condition_local(&decider);
        self.in_loop(span, guard, |walker| {
            if let Some(update) = update {
                walker.stmt(update)?;
            }
            walker.scoped(body)
        })?;
        match condition {
            Some(condition) => self.loop_decides(since, decider, guard, condition, span),
            None => Ok(()),
        }
    }

    fn for_of(
        &mut self,
        name: &Ident,
        element_ty: &Type,
        iter: ExprId,
        body: StmtId,
        span: Span,
    ) -> Result<(), CompilerFailure> {
        let iterated = self.eval(iter)?;
        let element = iterated
            .iter()
            .map(|term| term.read(&ReadKey::Iteration))
            .collect();
        let since = self.mark();
        self.scopes.push();
        let sources = sources_read(&element);
        let local = self.declare(name, element, is_primitive(element_ty));
        self.note_aliases(local, &sources, span, false);
        let guard = self.condition_local(&iterated);
        let walked = self.in_loop(span, guard, |walker| walker.stmt(body));
        self.scopes.pop();
        walked?;
        self.loop_decides(since, iterated, guard, iter, span)
    }

    fn catch(&mut self, clause: &TypedCatchClause) -> Result<(), CompilerFailure> {
        self.scopes.push();
        self.declare(&clause.binding, Flow::new(), false);
        let walked = self.stmt(clause.body);
        self.scopes.pop();
        walked
    }

    /// Walks a branch that the condition `guard` holds decides whether to
    /// run.
    fn guarded<T>(
        &mut self,
        guard: LocalId,
        walk: impl FnOnce(&mut Self) -> Result<T, CompilerFailure>,
    ) -> Result<T, CompilerFailure> {
        self.guards.push(guard);
        let walked = walk(self);
        self.guards.pop();
        walked
    }

    /// A local that holds what `decider` decides with, made once for its
    /// construct, so that each local assigned under it, or after a branch of
    /// it that left, refers to it instead of holding a copy.
    fn condition_local(&mut self, decider: &Flow) -> LocalId {
        self.new_local("a condition".to_string(), derived(decider.clone()), true)
    }

    /// Walks the body of the loop at `span`, which the condition `guard`
    /// holds decides whether to run.
    fn in_loop(
        &mut self,
        span: Span,
        guard: LocalId,
        walk: impl FnOnce(&mut Self) -> Result<(), CompilerFailure>,
    ) -> Result<(), CompilerFailure> {
        self.function.loops.push(span);
        self.function.breakables.push(Breakable::Loop);
        self.enclosing_loops.push(span);
        let walked = self.guarded(guard, walk);
        self.enclosing_loops.pop();
        self.function.breakables.pop();
        self.function.loops.pop();
        walked
    }

    /// What decides whether the code at this point runs: a reference to
    /// each condition's local.
    fn guard_terms(&self) -> Flow {
        self.guards
            .iter()
            .chain(&self.function.left_guards)
            .map(|condition| Term {
                derived: true,
                ..Term::of(Source::Local(*condition))
            })
            .collect()
    }

    fn mark(&self) -> Mark {
        Mark {
            checks_met: self.checks_met,
            exits: self.function.exits,
            returns: self.function.returns,
        }
    }

    /// The condition of a loop decides whether a `check()` runs when the
    /// loop holds one, or when its body leaves the function and a `check()`
    /// comes after the loop.
    fn loop_decides(
        &mut self,
        since: Mark,
        decider: Flow,
        guard: LocalId,
        condition: ExprId,
        span: Span,
    ) -> Result<(), CompilerFailure> {
        let holds_check = self.checks_met > since.checks_met;
        let returns = self.function.returns > since.returns;
        if returns {
            self.function.left_guards.push(guard);
        }
        if holds_check || returns && self.check_follows(span) {
            self.record_control(decider, condition)?;
        }
        Ok(())
    }

    /// The condition of a branching construct decides whether a `check()`
    /// runs when a branch holds one, or when a branch leaves and a `check()`
    /// comes after the construct or in a loop around it. Which calls the
    /// exit actually skips is not worked out: any such one counts.
    fn branches_decide(
        &mut self,
        since: Mark,
        decider: Flow,
        guard: LocalId,
        condition: ExprId,
        construct: Span,
    ) -> Result<(), CompilerFailure> {
        let holds_check = self.checks_met > since.checks_met;
        let leaves = self.function.exits > since.exits;
        if leaves {
            self.function.left_guards.push(guard);
        }
        let skips_check = leaves && (self.check_follows(construct) || self.in_checking_loop());
        if holds_check || skips_check {
            self.record_control(decider, condition)?;
        }
        Ok(())
    }

    fn check_follows(&self, construct: Span) -> bool {
        self.check_spans
            .iter()
            .any(|check| check.file == construct.file && check.start > construct.start)
    }

    fn in_checking_loop(&self) -> bool {
        self.function
            .loops
            .iter()
            .any(|span| self.holds_check(*span))
    }

    fn holds_check(&self, span: Span) -> bool {
        self.check_spans.iter().any(|check| contains(span, *check))
    }

    /// Whether what runs at `at` can run before a `check()`. Code after the
    /// last `check()` of the body, in no loop that holds a `check()` and not
    /// in a nested function, runs after every one.
    fn may_precede_check(&self, at: Span) -> bool {
        let Some(last) = self.last_check else {
            return true;
        };
        let after = at.file == last.file && at.start >= last.end;
        !after
            || self.closure_depth > 0
            || self
                .enclosing_loops
                .iter()
                .any(|span| self.holds_check(*span))
    }

    fn record_control(&mut self, decider: Flow, condition: ExprId) -> Result<(), CompilerFailure> {
        let span = self.package.expr_at(condition)?.span;
        self.sinks.push(Sink {
            flow: decider,
            span,
            kind: ReachKind::Control,
        });
        Ok(())
    }
}

// The walk: locals.
impl Walker<'_, '_> {
    fn declare(&mut self, name: &Ident, flow: Flow, primitive: bool) -> LocalId {
        let local = self.new_local(name.name.clone(), flow, primitive);
        self.scopes.declare(&name.name, local);
        local
    }

    /// A new local holding `flow`, which is charged to the budget; once the
    /// budget is gone it holds nothing, as the body is examined whole anyway.
    fn new_local(&mut self, name: String, flow: Flow, primitive: bool) -> LocalId {
        let local = LocalId(self.locals.len());
        let made_from = sources_read(&flow);
        let flow = if self.work.spend(flow.len()) {
            flow
        } else {
            Flow::new()
        };
        self.locals.push(Local {
            name,
            made_from,
            may_be_foreign: false,
            holds_foreign: false,
            flow,
            primitive,
            depth: self.closure_depth,
            loops: self.enclosing_loops.clone(),
            uses: Vec::new(),
        });
        local
    }

    /// The caller supplies the parameters of the body, and of `function`
    /// when it is a nested function that calls `check()`.
    fn declare_params(&mut self, params: &[TypedParam], function: Option<ExprId>) {
        for (position, param) in params.iter().enumerate() {
            let parameter = ParameterId { function, position };
            self.parameters
                .insert(parameter, parameter_shown(param, position));
            let root = Term::of(Source::Caller(ValueRoot::Parameter(parameter)));
            self.declare(&param.name, Flow::from([root]), is_primitive(&param.ty));
        }
    }

    fn local(&self, local: LocalId) -> Result<&Local, CompilerFailure> {
        self.locals.get(local.0).ok_or_else(undeclared)
    }

    fn local_mut(&mut self, local: LocalId) -> Result<&mut Local, CompilerFailure> {
        self.locals.get_mut(local.0).ok_or_else(undeclared)
    }

    fn used(&mut self, local: LocalId, span: Span) {
        let in_closure = self.closure_depth > 0;
        if let Some(declared) = self.locals.get_mut(local.0) {
            let captured = in_closure && declared.depth < self.closure_depth;
            declared.uses.push(LocalUse { span, captured });
        }
    }

    /// Whether `local` holds a container the body built: one it made, or one
    /// moved into it.
    fn is_owned(&self, local: LocalId) -> Result<bool, CompilerFailure> {
        Ok(match &self.ownership {
            Some(ownership) => ownership.owned.contains(&local),
            None => self.local(local)?.built_container(),
        })
    }

    /// Records each local of `sources` that `local` is made from, when
    /// `local` may share a container with it. A for-of element is part of
    /// the value iterated, so it is never a move.
    fn note_aliases(
        &mut self,
        local: LocalId,
        sources: &BTreeSet<Source>,
        span: Span,
        movable: bool,
    ) {
        let primitive = self
            .locals
            .get(local.0)
            .is_none_or(|declared| declared.primitive);
        if primitive || !self.may_precede_check(span) {
            return;
        }
        for source in sources {
            if let Source::Local(from) = source {
                self.events.push(Event::Alias {
                    from: *from,
                    into: local,
                    span,
                    movable,
                    loops: self.enclosing_loops.clone(),
                });
            }
        }
    }

    /// Assigns a local declared earlier: a new value it may be, and one a
    /// nested function may set behind the walk's back.
    fn reassign(&mut self, local: LocalId, flow: Flow, span: Span) -> Result<(), CompilerFailure> {
        self.used(local, span);
        if !self.may_precede_check(span) {
            return Ok(());
        }
        if self.local(local)?.depth < self.closure_depth {
            self.events.push(Event::NestedAssign { local, span });
        }
        let made_from = sources_read(&flow);
        self.note_aliases(local, &made_from, span, true);
        self.local_mut(local)?.made_from.extend(made_from);
        self.add_to(local, flow)
    }

    /// A later value is derived: the local's value no longer has the path
    /// it was declared with. What decides whether it runs decides the value
    /// as well.
    fn add_to(&mut self, local: LocalId, flow: Flow) -> Result<(), CompilerFailure> {
        let guards = self.guard_terms();
        if !self.work.spend(flow.len().saturating_add(guards.len())) {
            return Ok(());
        }
        let assigned = &mut self.local_mut(local)?.flow;
        assigned.extend(derived(flow));
        assigned.extend(guards);
        Ok(())
    }

    /// Writes `stored` into a field or element of the value `receiver`
    /// names: exactly into a container the body built, and as a reason to
    /// examine everything into a value the body does not own.
    /// `stores_foreign` says whether the value written holds an object the
    /// body does not own.
    fn write_into(
        &mut self,
        receiver: &Flow,
        stored: Flow,
        stores_foreign: bool,
        span: Span,
    ) -> Result<(), CompilerFailure> {
        if !self.may_precede_check(span) {
            return Ok(());
        }
        for term in receiver {
            if let Source::Local(local) = term.source
                && self.is_owned(local)?
            {
                self.add_to(local, stored.clone())?;
                if stores_foreign {
                    self.local_mut(local)?.holds_foreign = true;
                }
            }
        }
        if self.is_foreign(receiver)? {
            self.foreign_write("a value the body does not own".to_string(), span);
        }
        Ok(())
    }

    /// Whether `flow` may name a value the body does not own:
    /// - a value computed from nothing the walk tracks, such as a call's
    ///   result with no arguments, used directly;
    /// - any term from the caller's value, `this`, a global or a name with
    ///   no binding, even where it was computed with values the body owns;
    /// - a local made from one of those, or one that may be such an object
    ///   (see [`Local::may_be_foreign`]);
    /// - a field or element of a container the body built that holds such
    ///   an object.
    ///
    /// A container the body built is its own, and a local made from another
    /// local is covered by `Event::Alias`. A new value (see [`is_fresh`]) is
    /// the caller's to rule out.
    fn is_foreign(&mut self, flow: &Flow) -> Result<bool, CompilerFailure> {
        if flow.is_empty() {
            return Ok(true);
        }
        for term in flow {
            let foreign = match term.source {
                Source::Caller(_) => true,
                Source::Local(local) if self.is_owned(local)? => {
                    !term.steps.is_empty() && self.local(local)?.holds_foreign
                }
                Source::Local(local) => {
                    self.local(local)?.may_be_foreign || self.is_callers_value(local)?
                }
            };
            if foreign {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Whether `local` is, or is part of, a value the body did not make:
    /// the caller's, `this`, a global's, or a name's with no binding,
    /// through any chain of locals made from one another. A parameter is a
    /// local made from the caller's value. Once the budget is gone the
    /// answer is yes, which errs towards examining everything.
    fn is_callers_value(&mut self, local: LocalId) -> Result<bool, CompilerFailure> {
        let mut seen = BTreeSet::new();
        let mut pending = vec![local];
        while let Some(local) = pending.pop() {
            if !seen.insert(local) {
                continue;
            }
            if !self.work.spend(1) {
                return Ok(true);
            }
            for source in &self.local(local)?.made_from {
                match source {
                    Source::Caller(_) => return Ok(true),
                    Source::Local(from) => pending.push(*from),
                }
            }
        }
        Ok(false)
    }

    /// Whether `value` holds an object the body does not own: itself, or,
    /// for a literal, one of its members.
    fn passes_foreign_object(&mut self, value: ExprId) -> Result<bool, CompilerFailure> {
        let mut parts = Vec::new();
        self.parts_of(value, &mut parts)?;
        for part in &parts {
            if self.is_foreign(part)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Records what `local`, made from `value`, may be or hold: an object the
    /// body does not own, when `value` is not new and one of the objects it
    /// is made from carries the caller's data or a global's, and one inside
    /// it, when `value` passes one.
    fn note_made_from(&mut self, local: LocalId, value: ExprId) -> Result<(), CompilerFailure> {
        let fresh = is_fresh(self.package.ta, self.package.expr_at(value)?)?;
        let may_be_foreign = !fresh && self.made_from_foreign_object(value)?;
        let holds_foreign = self.passes_foreign_object(value)?;
        let made = self.local_mut(local)?;
        made.may_be_foreign |= may_be_foreign;
        made.holds_foreign |= holds_foreign;
        Ok(())
    }

    /// Whether `value` may be an object the body does not own: one of the
    /// objects it is made from (itself, or a call's receiver and arguments)
    /// carries the caller's data or a global's. A primitive holds no object,
    /// so a call made only with primitives, as `toBody(input.kind)`, makes
    /// the body's own value.
    fn made_from_foreign_object(&mut self, value: ExprId) -> Result<bool, CompilerFailure> {
        let expr = self.package.expr_at(value)?;
        let mut parts = Vec::new();
        match &expr.kind {
            TypedExprKind::Cast { value, .. }
            | TypedExprKind::NonNullAssert { value }
            | TypedExprKind::Narrowed { inner: value, .. } => {
                return self.made_from_foreign_object(*value);
            }
            TypedExprKind::Call { args, .. }
            | TypedExprKind::McpCall { args, .. }
            | TypedExprKind::IntrinsicCall { args, .. } => {
                for arg in self.package.authored(expr.span, args) {
                    self.parts_of(*arg, &mut parts)?;
                }
            }
            TypedExprKind::GenericCall { args, .. } => {
                for arg in args {
                    self.parts_of(arg.expr, &mut parts)?;
                }
            }
            TypedExprKind::CallClosure { callee, args } => {
                self.parts_of(*callee, &mut parts)?;
                for arg in self.package.authored(expr.span, args) {
                    self.parts_of(*arg, &mut parts)?;
                }
            }
            TypedExprKind::MethodCall { receiver, args, .. } => {
                self.parts_of(*receiver, &mut parts)?;
                for arg in self.package.authored(expr.span, args) {
                    self.parts_of(*arg, &mut parts)?;
                }
            }
            TypedExprKind::GenericMethodCall { receiver, args, .. } => {
                self.parts_of(*receiver, &mut parts)?;
                for arg in args {
                    self.parts_of(arg.expr, &mut parts)?;
                }
            }
            _ => self.parts_of(value, &mut parts)?,
        }
        for part in &parts {
            if self.carries_foreign(part)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Whether `flow` carries a value the body does not own: a term from
    /// the caller's value, `this`, a global or a name with no binding, or a
    /// local that is, may be, or holds such an object.
    fn carries_foreign(&mut self, flow: &Flow) -> Result<bool, CompilerFailure> {
        for term in flow {
            let carries = match term.source {
                Source::Caller(_) => true,
                Source::Local(local) => {
                    let made = self.local(local)?;
                    made.may_be_foreign || made.holds_foreign || self.is_callers_value(local)?
                }
            };
            if carries {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn foreign_write(&mut self, what: String, span: Span) {
        if self.may_precede_check(span) {
            self.events.push(Event::ForeignWrite { what, span });
        }
    }
}

// The walk: expressions.
impl<'a> Walker<'a, '_> {
    fn eval(&mut self, id: ExprId) -> Result<Flow, CompilerFailure> {
        if let Some(flow) = self.evaluated.get(&id) {
            return Ok(flow.clone());
        }
        let expr = self.package.expr_at(id)?;
        let flow = self.eval_kind(id, expr)?;
        self.evaluated.insert(id, flow.clone());
        Ok(flow)
    }

    fn eval_kind(&mut self, id: ExprId, expr: &'a TypedExpr) -> Result<Flow, CompilerFailure> {
        let span = expr.span;
        Ok(match &expr.kind {
            TypedExprKind::Number(_)
            | TypedExprKind::BigInt(_)
            | TypedExprKind::String(_)
            | TypedExprKind::Boolean(_)
            | TypedExprKind::Null
            | TypedExprKind::Regex { .. }
            | TypedExprKind::FunctionRef { .. }
            | TypedExprKind::NumberEnumMember { .. }
            | TypedExprKind::StringEnumMember { .. } => Flow::new(),
            TypedExprKind::This => self.this(),
            TypedExprKind::LocalRef { ident, .. } => self.local_ref(&ident.name, span),
            TypedExprKind::GlobalRef { mangled, .. } => self.global(mangled, expr),
            TypedExprKind::LocalNarrowRef { path, .. } => self.narrowed_reference(path, expr),
            TypedExprKind::Call { mangled, args, .. } => self.call(mangled, span, args)?,
            TypedExprKind::GenericCall { mangled, args, .. } => {
                let args: Vec<ExprId> = args.iter().map(|arg| arg.expr).collect();
                self.call(mangled, span, &args)?
            }
            TypedExprKind::McpCall { tool, args, .. } => {
                self.call_with(None, self.package.authored(span, args), tool, span)?
            }
            TypedExprKind::SuperCtorCall { args, .. } => {
                self.call_with(None, self.package.authored(span, args), "super", span)?
            }
            TypedExprKind::IntrinsicCall { kind, args } => {
                self.call_with(None, self.package.authored(span, args), kind.name(), span)?
            }
            TypedExprKind::SuperMethodCall { name, args, .. } => {
                let args = self.package.authored(span, args);
                self.method_call(self.this(), None, false, args, &name.name, span)?
            }
            TypedExprKind::CallClosure { callee, args } => {
                let (name, calls_itself) = self.function_value_name(*callee)?;
                let callee = self.eval(*callee)?;
                let args = self.package.authored(span, args);
                self.call_function_value(callee, name, calls_itself, args, span)?
            }
            TypedExprKind::MethodCall {
                receiver,
                name,
                args,
                ..
            } => {
                let args = self.package.authored(span, args);
                self.method_call_on(*receiver, args, &name.name, span)?
            }
            TypedExprKind::GenericMethodCall {
                receiver,
                name,
                args,
                ..
            } => {
                let args: Vec<ExprId> = args.iter().map(|arg| arg.expr).collect();
                let args = self.package.authored(span, &args);
                self.method_call_on(*receiver, args, &name.name, span)?
            }
            TypedExprKind::Binary { op, lhs, rhs } => self.binary(*op, *lhs, *rhs, span)?,
            TypedExprKind::NullishCoalesce { lhs, rhs } => self.either(*lhs, *rhs, span)?,
            TypedExprKind::Ternary { cond, then_, else_ } => {
                let decider = identity(self.eval(*cond)?);
                let since = self.mark();
                let guard = self.condition_local(&decider);
                let mut flow = self.guarded(guard, |walker| {
                    let mut flow = walker.eval(*then_)?;
                    flow.extend(walker.eval(*else_)?);
                    Ok(flow)
                })?;
                self.branches_decide(since, decider.clone(), guard, *cond, span)?;
                // The condition chooses the value.
                flow.extend(decider);
                flow
            }
            TypedExprKind::Unary { op, operand } => {
                let operand = self.eval(*operand)?;
                match op {
                    UnOp::Not => identity(operand),
                    UnOp::Neg | UnOp::Pos | UnOp::BitNot => derived(operand),
                }
            }
            TypedExprKind::TypeofTag { value, .. } | TypedExprKind::InstanceOf { value, .. } => {
                identity(self.eval(*value)?)
            }
            TypedExprKind::EffectThen { effect, result } => {
                self.eval(*effect)?;
                self.eval(*result)?
            }
            TypedExprKind::Sequence { stmts, result } => {
                for stmt in stmts {
                    self.stmt(*stmt)?;
                }
                self.eval(*result)?
            }
            // `source` is not walked: see `NarrowRegion`.
            TypedExprKind::Narrowed { inner: value, .. }
            | TypedExprKind::NonNullAssert { value }
            | TypedExprKind::Cast { value, .. } => self.eval(*value)?,
            TypedExprKind::FieldAccess { receiver, name }
            | TypedExprKind::InterfacePropertyAccess { receiver, name, .. } => {
                let receiver = self.eval(*receiver)?;
                read(&receiver, &ReadKey::Property(name.name.clone()))
            }
            TypedExprKind::IndexAccess { receiver, index } => self.index(*receiver, *index)?,
            TypedExprKind::OptionalChain { base, parts } => self.optional_chain(*base, parts)?,
            TypedExprKind::ObjectLiteral { members, .. } => {
                let mut flow = Flow::new();
                for member in members {
                    for value in member.expressions() {
                        flow.extend(self.eval(value)?);
                    }
                }
                derived(flow)
            }
            TypedExprKind::ArrayLiteral { elements, .. } => {
                let mut flow = Flow::new();
                for element in elements {
                    flow.extend(self.eval(element.expr_id())?);
                }
                derived(flow)
            }
            TypedExprKind::TupleLiteral { elements, .. } => {
                let mut flow = Flow::new();
                for element in elements {
                    flow.extend(self.eval(*element)?);
                }
                derived(flow)
            }
            TypedExprKind::Closure { params, body, .. } => self.closure(id, params, body)?,
            TypedExprKind::PostfixUnary { target, .. } => self.postfix(target, span)?,
        })
    }

    fn this(&self) -> Flow {
        if !self.body.has_receiver {
            return Flow::new();
        }
        Flow::from([Term::of(Source::Caller(ValueRoot::This))])
    }

    fn local_ref(&mut self, name: &str, span: Span) -> Flow {
        let source = match self.scopes.resolve(name).copied() {
            Some(local) => {
                let captured = self
                    .locals
                    .get(local.0)
                    .is_some_and(|declared| declared.depth < self.closure_depth);
                if captured {
                    self.events.push(Event::Captured { local, span });
                }
                self.used(local, span);
                Source::Local(local)
            }
            None => Source::Caller(ValueRoot::Unresolved(name.to_string())),
        };
        Flow::from([Term::of(source)])
    }

    /// A global that may hold anything. A primitive no code can rebind, a
    /// constant of this package or another, cannot change, and the runtime's
    /// own bindings hold nothing.
    fn global(&self, mangled: &MangledName, expr: &TypedExpr) -> Flow {
        let constant = is_primitive(&expr.ty) && !self.package.is_rebindable(mangled);
        if is_runtime_symbol(mangled) || constant {
            return Flow::new();
        }
        Flow::from([Term::of(Source::Caller(ValueRoot::Global(mangled.clone())))])
    }

    fn narrowed_reference(&mut self, path: &ReferencePath, expr: &TypedExpr) -> Flow {
        let root = match &path.root {
            BindingId::Local { name, .. } => self.local_ref(name, expr.span),
            BindingId::Global(mangled) => self.global(mangled, expr),
            BindingId::This => self.this(),
        };
        path.chain.iter().fold(root, |flow, element| {
            read(&flow, &ReadKey::of_path(element))
        })
    }

    fn binary(
        &mut self,
        op: BinOp,
        lhs: ExprId,
        rhs: ExprId,
        span: Span,
    ) -> Result<Flow, CompilerFailure> {
        match op {
            BinOp::And | BinOp::Or | BinOp::NullishCoalesce => self.either(lhs, rhs, span),
            BinOp::Eq | BinOp::NotEq => {
                let by_identity = self.is_null(lhs)?
                    || self.is_null(rhs)?
                    || compared_by_identity(&self.package.expr_at(lhs)?.ty)
                        && compared_by_identity(&self.package.expr_at(rhs)?.ty);
                let mut flow = self.eval(lhs)?;
                flow.extend(self.eval(rhs)?);
                Ok(if by_identity {
                    identity(flow)
                } else {
                    derived(flow)
                })
            }
            BinOp::Lt
            | BinOp::Gt
            | BinOp::Le
            | BinOp::Ge
            | BinOp::In
            | BinOp::Add
            | BinOp::Sub
            | BinOp::Mul
            | BinOp::Div
            | BinOp::Rem
            | BinOp::Pow
            | BinOp::BitAnd
            | BinOp::BitOr
            | BinOp::BitXor
            | BinOp::Shl
            | BinOp::Shr
            | BinOp::UnsignedShr => {
                let mut flow = self.eval(lhs)?;
                flow.extend(self.eval(rhs)?);
                Ok(derived(flow))
            }
        }
    }

    /// `&&`, `||` and `??` yield either operand, and the first decides, by
    /// its truthiness, whether the second runs.
    fn either(
        &mut self,
        first: ExprId,
        second: ExprId,
        construct: Span,
    ) -> Result<Flow, CompilerFailure> {
        let first_yield = self.eval(first)?;
        let decider = identity(first_yield.clone());
        let since = self.mark();
        let guard = self.condition_local(&decider);
        let mut flow = self.guarded(guard, |walker| walker.eval(second))?;
        self.branches_decide(since, decider, guard, first, construct)?;
        flow.extend(first_yield);
        Ok(flow)
    }

    fn is_null(&self, id: ExprId) -> Result<bool, CompilerFailure> {
        Ok(matches!(
            self.package.expr_at(id)?.kind,
            TypedExprKind::Null
        ))
    }

    /// The index chooses the element, so it flows with it.
    fn index(&mut self, receiver: ExprId, index: ExprId) -> Result<Flow, CompilerFailure> {
        let key = self.package.index_key(index)?;
        let receiver = self.eval(receiver)?;
        let mut flow = read(&receiver, &key);
        flow.extend(derived(self.eval(index)?));
        Ok(flow)
    }

    fn optional_chain(
        &mut self,
        base: ExprId,
        parts: &[TypedChainPart],
    ) -> Result<Flow, CompilerFailure> {
        let mut flow = self.eval(base)?;
        let base_expr = self.package.expr_at(base)?;
        let mut current_ty = &base_expr.ty;
        let mut fresh = is_fresh(self.package.ta, base_expr)?;
        for (position, part) in parts.iter().enumerate() {
            let receiver_ty = current_ty;
            let receiver_fresh = fresh;
            fresh = matches!(
                part,
                TypedChainPart::MethodCall { name, .. } if copies(receiver_ty, &name.name)
            );
            flow = match part {
                TypedChainPart::Field { name, .. }
                | TypedChainPart::InterfaceProperty { name, .. } => {
                    read(&flow, &ReadKey::Property(name.name.clone()))
                }
                TypedChainPart::Index { idx, .. } => {
                    let key = self.package.index_key(*idx)?;
                    let mut element = read(&flow, &key);
                    element.extend(derived(self.eval(*idx)?));
                    element
                }
                TypedChainPart::Call { args, span, .. } => {
                    let (name, calls_itself) = match position {
                        0 => self.function_value_name(base)?,
                        _ => ("a function value".to_string(), false),
                    };
                    let args = self.package.authored(*span, args);
                    self.call_function_value(flow, name, calls_itself, args, *span)?
                }
                TypedChainPart::MethodCall {
                    name, args, span, ..
                } => {
                    let args = self.package.authored(*span, args);
                    let changeable = (!is_primitive(receiver_ty)).then_some(receiver_ty);
                    self.method_call(flow, changeable, receiver_fresh, args, &name.name, *span)?
                }
                TypedChainPart::NonNull { .. } => flow,
            };
            current_ty = part.result_ty();
        }
        Ok(flow)
    }

    /// A function value yields what it returns when called. Its `return`,
    /// `throw`, `break` and `continue` leave it, not the function around it.
    fn closure(
        &mut self,
        id: ExprId,
        params: &[TypedParam],
        body: &ClosureBody,
    ) -> Result<Flow, CompilerFailure> {
        let outer = std::mem::take(&mut self.function);
        self.closure_depth = self.closure_depth.saturating_add(1);
        self.scopes.push();
        if let Some(name) = self.package.ta.closure_names.get(&id) {
            self.declare(name, Flow::new(), false);
        }
        if self.checking_closures.contains(&id) {
            self.declare_params(params, Some(id));
        } else {
            // Whoever calls the nested function supplies its parameters, so
            // an object among them may be one the body does not own.
            for param in params {
                let primitive = is_primitive(&param.ty);
                let local = self.declare(&param.name, Flow::new(), primitive);
                self.local_mut(local)?.may_be_foreign = !primitive;
            }
        }
        let walked = match body {
            ClosureBody::Expr(value) => self
                .eval(*value)
                .map(|flow| self.function.returned.extend(flow)),
            ClosureBody::Block(block) => self.stmt(*block),
        };
        self.scopes.pop();
        self.closure_depth = self.closure_depth.saturating_sub(1);
        let inner = std::mem::replace(&mut self.function, outer);
        walked?;
        Ok(derived(inner.returned))
    }

    fn postfix(&mut self, target: &PostfixTarget, span: Span) -> Result<Flow, CompilerFailure> {
        Ok(derived(match target {
            PostfixTarget::Local { ident, .. } => {
                let flow = self.local_ref(&ident.name, span);
                if let Some(local) = self.scopes.resolve(&ident.name).copied() {
                    self.reassign(local, Flow::new(), span)?;
                }
                flow
            }
            PostfixTarget::Global { mangled, .. } => {
                let global = self.package.global_shown(mangled);
                self.foreign_write(format!("the global `{global}`"), span);
                Flow::from([Term::of(Source::Caller(ValueRoot::Global(mangled.clone())))])
            }
            PostfixTarget::Field { receiver, name, .. } => {
                let receiver = self.eval(*receiver)?;
                self.write_into(&receiver, Flow::new(), false, span)?;
                read(&receiver, &ReadKey::Property(name.name.clone()))
            }
            PostfixTarget::Index {
                receiver, index, ..
            } => {
                let receiver_flow = self.eval(*receiver)?;
                self.write_into(&receiver_flow, Flow::new(), false, span)?;
                self.index(*receiver, *index)?
            }
        }))
    }
}

// The walk: calls.
impl Walker<'_, '_> {
    /// A call of a named function. The arguments of `check()` reach it; any
    /// other callee is called as [`Walker::call_with`] says, and a call of
    /// the body itself is recursion.
    fn call(
        &mut self,
        mangled: &MangledName,
        span: Span,
        args: &[ExprId],
    ) -> Result<Flow, CompilerFailure> {
        let args = self.package.authored(span, args);
        if !crate::stdlib::security::is_check(mangled) {
            if self.body.mangled.as_ref() == Some(mangled) {
                self.events.push(Event::Recursion { span });
            }
            return self.call_with(None, args, source_name(mangled), span);
        }
        self.checks_met = self.checks_met.saturating_add(1);
        for arg in args {
            let flow = self.eval(*arg)?;
            let span = self.package.expr_at(*arg)?.span;
            self.sinks.push(Sink {
                flow,
                span,
                kind: ReachKind::Data,
            });
        }
        Ok(Flow::new())
    }

    fn method_call_on(
        &mut self,
        receiver: ExprId,
        args: &[ExprId],
        method: &str,
        span: Span,
    ) -> Result<Flow, CompilerFailure> {
        let receiver_expr = self.package.expr_at(receiver)?;
        let flow = self.eval(receiver)?;
        let on_this = matches!(receiver_expr.kind, TypedExprKind::This);
        if on_this && self.member_name.as_deref() == Some(method) {
            self.events.push(Event::Recursion { span });
        }
        let holds_nothing = is_primitive(&receiver_expr.ty) || is_runtime_global(receiver_expr);
        let changeable = (!holds_nothing).then_some(&receiver_expr.ty);
        let fresh = is_fresh(self.package.ta, receiver_expr)?;
        self.method_call(flow, changeable, fresh, args, method, span)
    }

    /// A method call on the value `receiver` names. `changeable` is the
    /// receiver's type when the receiver can hold anything; the call takes
    /// the receiver as a value it may change only when the method may
    /// change a value of that type. `fresh` says the receiver is a new
    /// value (see [`is_fresh`]).
    fn method_call(
        &mut self,
        receiver: Flow,
        changeable: Option<&Type>,
        fresh: bool,
        args: &[ExprId],
        method: &str,
        span: Span,
    ) -> Result<Flow, CompilerFailure> {
        let changed = changeable
            .filter(|ty| may_change(ty, method))
            .map(|_| Changed {
                flow: &receiver,
                fresh,
            });
        let mut result = self.call_with(changed, args, method, span)?;
        result.extend(derived(receiver.clone()));
        Ok(result)
    }

    /// The name a call of a function value shows, and whether it calls the
    /// body itself.
    fn function_value_name(&self, callee: ExprId) -> Result<(String, bool), CompilerFailure> {
        Ok(match &self.package.expr_at(callee)?.kind {
            TypedExprKind::LocalRef { ident, .. } => {
                let own_name = self
                    .body
                    .function_value
                    .and_then(|function| self.package.ta.closure_names.get(&function));
                let own =
                    own_name.is_some_and(|own| own.name == ident.name) && self.closure_depth == 0;
                (ident.name.clone(), own)
            }
            TypedExprKind::GlobalRef { mangled, name } => (
                name.name.clone(),
                self.body.mangled.as_ref() == Some(mangled),
            ),
            _ => ("a function value".to_string(), false),
        })
    }

    /// A call of a function value: a nested function, or one a local or a
    /// global holds. What it does is not followed, so its result is held by
    /// a local of its own that tells whether it reaches `check()`.
    fn call_function_value(
        &mut self,
        callee: Flow,
        name: String,
        calls_itself: bool,
        args: &[ExprId],
        span: Span,
    ) -> Result<Flow, CompilerFailure> {
        if calls_itself {
            self.events.push(Event::Recursion { span });
        }
        let passed = self.call_with(None, args, &name, span)?;
        let mut flow = callee;
        flow.extend(passed.iter().cloned());
        let result = self.new_local(name.clone(), derived(flow), false);
        if self.may_precede_check(span) {
            self.events.push(Event::FunctionValueCall {
                result,
                args: passed,
                callee: name,
                span,
            });
        }
        Ok(Flow::from([Term::of(Source::Local(result))]))
    }

    /// A call to anything but `check()`. Its result carries the arguments
    /// and `changed`, the receiver when the method may change it. A
    /// container the body built, passed as an argument or as that receiver,
    /// takes all of them; passed with a caller's object, or a receiver the
    /// body does not own, the call is recorded as a reason to examine the
    /// whole body.
    fn call_with(
        &mut self,
        changed: Option<Changed<'_>>,
        args: &[ExprId],
        callee: &str,
        span: Span,
    ) -> Result<Flow, CompilerFailure> {
        let mut passed = Flow::new();
        let mut parts = Vec::new();
        for arg in args {
            passed.extend(self.eval(*arg)?);
            self.parts_of(*arg, &mut parts)?;
        }
        if let Some(receiver) = &changed {
            passed.extend(receiver.flow.iter().cloned());
            parts.push(receiver.flow.clone());
        }
        if !self.may_precede_check(span) {
            return Ok(derived(passed));
        }
        let mut owned = Vec::new();
        let mut others = Vec::new();
        for part in &parts {
            let (mine, theirs) = self.split_owned(part)?;
            owned.extend(mine);
            if !theirs.is_empty() {
                others.push(theirs);
            }
        }
        let mut others_foreign = false;
        for other in &others {
            others_foreign |= self.is_foreign(other)?;
        }
        for local in &owned {
            self.add_to(*local, passed.clone())?;
            if others_foreign {
                self.local_mut(*local)?.holds_foreign = true;
            }
        }
        if !owned.is_empty() && !others.is_empty() {
            self.events.push(Event::PassedWith {
                owned,
                others,
                callee: callee.to_string(),
                span,
            });
        }
        if let Some(receiver) = changed
            && !args.is_empty()
            && !receiver.fresh
            && self.is_foreign(receiver.flow)?
        {
            self.foreign_write(format!("the receiver of `{callee}`"), span);
        }
        Ok(derived(passed))
    }

    /// The flows of the objects an argument passes: itself, or, for a
    /// literal, each of its members, since a literal is a new object and a
    /// primitive member can hold nothing.
    fn parts_of(&mut self, arg: ExprId, parts: &mut Vec<Flow>) -> Result<(), CompilerFailure> {
        let expr = self.package.expr_at(arg)?;
        match &expr.kind {
            TypedExprKind::ObjectLiteral { members, .. } => {
                for member in members {
                    for value in member.expressions() {
                        self.parts_of(value, parts)?;
                    }
                }
            }
            TypedExprKind::ArrayLiteral { elements, .. } => {
                for element in elements {
                    self.parts_of(element.expr_id(), parts)?;
                }
            }
            TypedExprKind::TupleLiteral { elements, .. } => {
                for element in elements {
                    self.parts_of(*element, parts)?;
                }
            }
            _ if is_primitive(&expr.ty) => {}
            _ => parts.push(self.eval(arg)?),
        }
        Ok(())
    }

    /// The containers the body built that `flow` names, and the rest.
    fn split_owned(&self, flow: &Flow) -> Result<(Vec<LocalId>, Flow), CompilerFailure> {
        let mut owned = Vec::new();
        let mut rest = Flow::new();
        for term in flow {
            match &term.source {
                Source::Local(local) if self.is_owned(*local)? => owned.push(*local),
                _ => {
                    rest.insert(term.clone());
                }
            }
        }
        Ok((owned, rest))
    }
}

/// Whether calling `method` on a value of type `ty` may change the value.
/// For a union, whether it may change any member; `null` and primitives
/// never change.
fn may_change(ty: &Type, method: &str) -> bool {
    match ty.peel() {
        Type::Union(members) => members.iter().any(|member| may_change(member, method)),
        Type::Array(_) | Type::Tuple(_) => ARRAY_MUTATORS.contains(&method),
        Type::Uint8Array => BYTES_MUTATORS.contains(&method),
        Type::InterfaceRef { mangled, .. } => match mangled.as_str() {
            "submilli:prelude#Map" | "submilli:prelude#Set" => {
                COLLECTION_MUTATORS.contains(&method)
            }
            "submilli:prelude#RegExp"
            | "submilli:prelude#TextEncoder"
            | "submilli:prelude#TextDecoder" => false,
            name => !name.starts_with("submilli:prelude#Temporal#"),
        },
        _ => !is_primitive(ty),
    }
}

/// Whether `expr` makes a new value, which no one else holds: a literal, a
/// builtin constructor (`new` of a builtin such as `new Map()` or
/// `new Uint8Array(n)`, `Array.from`, `Object.keys` and the like), or a
/// builtin method that returns a copy (`slice`, `map`, `split` and the
/// like), or `JSON.parse`. `Object.assign` returns its first argument, so it
/// is not one, and `new` of a package class is a call of its constructor.
fn is_fresh(ta: &crate::TypedAst, expr: &TypedExpr) -> Result<bool, CompilerFailure> {
    Ok(match &expr.kind {
        TypedExprKind::ArrayLiteral { .. }
        | TypedExprKind::ObjectLiteral { .. }
        | TypedExprKind::TupleLiteral { .. }
        | TypedExprKind::IntrinsicCall {
            kind: crate::Intrinsic::JsonParse,
            ..
        } => true,
        TypedExprKind::Cast { value, .. }
        | TypedExprKind::NonNullAssert { value }
        | TypedExprKind::Narrowed { inner: value, .. } => is_fresh(
            ta,
            ta.try_expr(*value)
                .map_err(crate::typechecker::arena_failure)?,
        )?,
        TypedExprKind::MethodCall {
            receiver,
            iface,
            name,
            ..
        }
        | TypedExprKind::GenericMethodCall {
            receiver,
            iface,
            name,
            ..
        } => {
            let receiver = ta
                .try_expr(*receiver)
                .map_err(crate::typechecker::arena_failure)?;
            let constructs = iface.as_str().ends_with("Constructor")
                && CONSTRUCTORS.contains(&name.name.as_str());
            constructs || copies(&receiver.ty, &name.name)
        }
        _ => false,
    })
}

/// Whether `===` and its kin compare values of type `ty` by identity: a
/// `Map`, a `Set` or a regular expression, or `null`. Arrays, objects and
/// class instances compare by their stored contents.
fn compared_by_identity(ty: &Type) -> bool {
    match ty.peel() {
        Type::Null => true,
        Type::Union(members) => members.iter().all(compared_by_identity),
        Type::InterfaceRef { mangled, .. } => matches!(
            mangled.as_str(),
            "submilli:prelude#Map" | "submilli:prelude#Set" | "submilli:prelude#RegExp"
        ),
        _ => false,
    }
}

/// Whether calling `method` on a value of type `ty` returns a copy. For a
/// union every member must copy, where [`may_change`] asks whether any
/// member may change: in doubt the value stays foreign.
fn copies(ty: &Type, method: &str) -> bool {
    match ty.peel() {
        Type::Union(members) => members.iter().all(|member| copies(member, method)),
        Type::Array(_) | Type::Tuple(_) => ARRAY_COPIES.contains(&method),
        Type::String | Type::StringLiteral(_) => STRING_COPIES.contains(&method),
        _ => false,
    }
}

/// An alias that is a move: its source, its destination, and where it is.
type Move = (LocalId, LocalId, (u32, u32, u32));

/// The containers the body built, once moves are known.
struct Ownership {
    owned: BTreeSet<LocalId>,
    /// The aliases that are moves.
    moves: BTreeSet<Move>,
}

// Moves: which aliases pass a container on rather than share it.
impl Walker<'_, '_> {
    /// The aliases whose source is never used again: the source's container
    /// moves to the new local rather than being shared with it.
    fn moves(&self) -> BTreeSet<Move> {
        let mut moves = BTreeSet::new();
        for event in &self.events {
            let Event::Alias {
                from,
                into,
                span,
                movable: true,
                loops,
            } = event
            else {
                continue;
            };
            let Some(source) = self.locals.get(from.0) else {
                continue;
            };
            if !source
                .uses
                .iter()
                .any(|used| used_again(used, *span, loops, source))
            {
                moves.insert((*from, *into, position(*span)));
            }
        }
        moves
    }

    /// Which locals hold a container the body built: those that made one,
    /// and those every value of which was moved from such a local.
    fn ownership(&self, moves: BTreeSet<Move>) -> Ownership {
        let moved: BTreeSet<(LocalId, LocalId)> =
            moves.iter().map(|(from, into, _)| (*from, *into)).collect();
        let mut owned: BTreeSet<LocalId> = (0..self.locals.len())
            .map(LocalId)
            .filter(|local| self.locals.get(local.0).is_some_and(Local::built_container))
            .collect();
        loop {
            let mut grew = false;
            for (index, local) in self.locals.iter().enumerate() {
                let id = LocalId(index);
                if local.primitive || local.made_from.is_empty() || owned.contains(&id) {
                    continue;
                }
                let all_moved = local.made_from.iter().all(|source| match source {
                    Source::Local(from) => owned.contains(from) && moved.contains(&(*from, id)),
                    Source::Caller(_) => false,
                });
                if all_moved {
                    owned.insert(id);
                    grew = true;
                }
            }
            if !grew {
                return Ownership { owned, moves };
            }
        }
    }
}

/// Whether `used` uses a local again after an alias made from it at
/// `alias`, in `loops`. A capture may run at any time; a use outside the
/// alias's own statement comes after it in source order, or runs again in
/// a loop around the alias that the local's declaration is not in.
fn used_again(used: &LocalUse, alias: Span, loops: &[Span], source: &Local) -> bool {
    if used.captured {
        return true;
    }
    if contains(alias, used.span) {
        return false;
    }
    let after = used.span.file == alias.file && used.span.start >= alias.end;
    let rerun = loops
        .iter()
        .filter(|span| !source.loops.contains(span))
        .any(|span| contains(*span, used.span));
    after || rerun
}

// The verdict: what reaches `check()`, and whether the body is complex.
impl Walker<'_, '_> {
    fn finish(mut self) -> Result<Checked, CompilerFailure> {
        let label = self.body.label.clone();
        let complex = |span: Span, reason: String| Checked {
            reaches: Vec::new(),
            complex: Some(Complexity {
                span,
                reason: format!(
                    "{reason}, so every caller-supplied value in `{label}` is examined"
                ),
            }),
        };
        let too_large = format!("`{label}` is too large to follow what reaches `check()`");
        if self.work.exceeded {
            return Ok(complex(self.body.label_span, too_large));
        }
        let Some(held) = resolve(&self.locals, &mut self.work)? else {
            return Ok(complex(self.body.label_span, too_large));
        };
        let (earliest, mut reasons) = self.earliest_reaches(&held)?;
        let Some(reaching) = self.reaching_locals()? else {
            return Ok(complex(self.body.label_span, too_large));
        };
        for event in &self.events {
            if let Some(reason) = self.reason(event, &reaching, &held)? {
                reasons.push(reason);
            }
        }
        if let Some((span, reason)) = reasons.into_iter().min_by_key(|(span, _)| position(*span)) {
            return Ok(complex(span, reason));
        }
        let reaches = earliest
            .into_iter()
            .map(|((origin, reliance), (span, kind))| Reach {
                shown: self.shown(&origin),
                origin,
                reliance,
                span,
                kind,
            })
            .collect();
        Ok(Checked {
            reaches,
            complex: None,
        })
    }

    /// Where each value first reaches `check()`, and why the body is
    /// complex when shared state does.
    fn earliest_reaches(
        &self,
        held: &[BTreeSet<Held>],
    ) -> Result<(Earliest, Vec<(Span, String)>), CompilerFailure> {
        let mut earliest = Earliest::new();
        let mut reasons = Vec::new();
        for sink in &self.sinks {
            for term in &sink.flow {
                for value in resolve_term(term, held)? {
                    if self.is_shared_root(&value.origin.root) {
                        let shown = self.shown(&value.origin);
                        reasons.push((
                            sink.span,
                            format!(
                                "`check()` depends on `{shown}`, which code outside `{}` can change",
                                self.body.label
                            ),
                        ));
                    }
                    let key = (value.origin, value.reliance);
                    let earlier = earliest
                        .get(&key)
                        .is_some_and(|(span, _)| position(*span) <= position(sink.span));
                    if !earlier {
                        earliest.insert(key, (sink.span, sink.kind));
                    }
                }
            }
        }
        Ok((earliest, reasons))
    }

    /// Whether code outside the body can change what `root` names: `this`,
    /// a module `let`, a writable static field, another package's global, or
    /// a name with no binding.
    fn is_shared_root(&self, root: &ValueRoot) -> bool {
        match root {
            ValueRoot::Parameter(_) => false,
            ValueRoot::This | ValueRoot::Unresolved(_) => true,
            ValueRoot::Global(mangled) if is_static_field(self.package.ta, mangled) => {
                self.package.is_rebindable(mangled)
            }
            ValueRoot::Global(mangled) => !self.is_module_const(mangled),
        }
    }

    fn is_module_const(&self, mangled: &MangledName) -> bool {
        is_module_const(self.package.ta, mangled)
    }

    /// The locals whose value, or part of it, reaches `check()`. `None` when
    /// finding them takes more than the budget left.
    fn reaching_locals(&mut self) -> Result<Option<BTreeSet<LocalId>>, CompilerFailure> {
        let mut reaching = BTreeSet::new();
        let mut pending: Vec<LocalId> = self
            .sinks
            .iter()
            .flat_map(|sink| &sink.flow)
            .filter_map(term_local)
            .collect();
        while let Some(local) = pending.pop() {
            if !reaching.insert(local) {
                continue;
            }
            let flow = &self.locals.get(local.0).ok_or_else(undeclared)?.flow;
            if !self.work.spend(flow.len()) {
                return Ok(None);
            }
            pending.extend(flow.iter().filter_map(term_local));
        }
        Ok(Some(reaching))
    }

    /// Why `event` makes the body complex, when it does.
    fn reason(
        &self,
        event: &Event,
        reaching: &BTreeSet<LocalId>,
        held: &[BTreeSet<Held>],
    ) -> Result<Option<(Span, String)>, CompilerFailure> {
        let label = &self.body.label;
        let depends = |local: LocalId, why: &str| -> Result<String, CompilerFailure> {
            Ok(format!(
                "`check()` depends on `{}`, which {why}",
                self.local(local)?.name
            ))
        };
        Ok(Some(match event {
            Event::Alias {
                from, into, span, ..
            } => {
                let moved = self.ownership.as_ref().is_some_and(|ownership| {
                    ownership.moves.contains(&(*from, *into, position(*span)))
                });
                let reaches = reaching.contains(from) || reaching.contains(into);
                if moved || !reaches || !self.is_owned(*from)? {
                    return Ok(None);
                }
                let into = &self.local(*into)?.name;
                (*span, depends(*from, &format!("`{into}` is made from"))?)
            }
            Event::Captured { local, span } => {
                if !reaching.contains(local) || !self.is_owned(*local)? {
                    return Ok(None);
                }
                (*span, depends(*local, "a nested function captures")?)
            }
            Event::NestedAssign { local, span } => {
                if !reaching.contains(local) {
                    return Ok(None);
                }
                (*span, depends(*local, "a nested function assigns")?)
            }
            Event::PassedWith {
                owned,
                others,
                callee,
                span,
            } => {
                let mut container = None;
                for local in owned {
                    if reaching.contains(local) && self.is_owned(*local)? {
                        container = Some(*local);
                        break;
                    }
                }
                let Some(container) = container else {
                    return Ok(None);
                };
                let Some(caller) = self.callers_object(others, held)? else {
                    return Ok(None);
                };
                let why = format!("is passed to `{callee}` with `{caller}`");
                (*span, depends(container, &why)?)
            }
            Event::FunctionValueCall {
                result,
                args,
                callee,
                span,
            } => {
                let passes_reaching = args
                    .iter()
                    .filter_map(term_local)
                    .any(|local| reaching.contains(&local));
                if !reaching.contains(result) && !passes_reaching {
                    return Ok(None);
                }
                (
                    *span,
                    format!(
                        "`check()` depends on a call of `{callee}`, whose effects are not followed"
                    ),
                )
            }
            Event::ForeignWrite { what, span } => {
                (*span, format!("`{label}` changes {what} before `check()`"))
            }
            Event::Recursion { span } => (*span, format!("`{label}` calls itself")),
        }))
    }

    /// The first of `others` that names a caller's object, as a message
    /// shows it. A module constant is the package's, not the caller's.
    fn callers_object(
        &self,
        others: &[Flow],
        held: &[BTreeSet<Held>],
    ) -> Result<Option<String>, CompilerFailure> {
        for term in others.iter().flatten() {
            for value in resolve_term(term, held)? {
                let constant = matches!(
                    &value.origin.root,
                    ValueRoot::Global(mangled) if self.is_module_const(mangled)
                );
                if !constant {
                    return Ok(Some(self.shown(&value.origin)));
                }
            }
        }
        Ok(None)
    }

    fn shown(&self, origin: &Origin) -> String {
        let root = match &origin.root {
            ValueRoot::Parameter(parameter) => self
                .parameters
                .get(parameter)
                .map_or("a parameter", String::as_str),
            ValueRoot::This => "this",
            ValueRoot::Global(mangled) => self.package.global_shown(mangled),
            ValueRoot::Unresolved(name) => name,
        };
        origin
            .steps
            .iter()
            .fold(root.to_string(), |shown, key| key.shown_on(&shown))
    }
}

/// What each local may hold, by [`LocalId`]; `None` when resolving it takes
/// more than the budget left.
///
/// A local's set only grows, and only a declaration extends a path, which
/// refers to locals declared before it, so the sets settle. Every pass
/// spends at least one step until they do, so the budget bounds the loop.
fn resolve(
    locals: &[Local],
    work: &mut Work,
) -> Result<Option<Vec<BTreeSet<Held>>>, CompilerFailure> {
    let mut held: Vec<BTreeSet<Held>> = vec![BTreeSet::new(); locals.len()];
    loop {
        let mut changed = false;
        for (index, local) in locals.iter().enumerate() {
            let mut resolved = Vec::new();
            for term in &local.flow {
                if !work.spend(resolution_cost(term, &held)?) {
                    return Ok(None);
                }
                resolved.extend(resolve_term(term, &held)?);
            }
            let slot = held.get_mut(index).ok_or_else(undeclared)?;
            let before = slot.len();
            slot.extend(resolved);
            changed |= slot.len() != before;
        }
        if !changed {
            return Ok(Some(held));
        }
    }
}

/// The steps resolving `term` takes: one for the term, and for each value
/// it yields one plus one per read in its path. A caller's value yields one
/// value, whose path is the term's `reads`, hence `reads + 2`.
fn resolution_cost(term: &Term, held: &[BTreeSet<Held>]) -> Result<usize, CompilerFailure> {
    let reads = term.steps.len();
    Ok(match &term.source {
        Source::Caller(_) => reads.saturating_add(2),
        Source::Local(LocalId(index)) => held
            .get(*index)
            .ok_or_else(undeclared)?
            .iter()
            .map(|value| {
                value
                    .origin
                    .steps
                    .len()
                    .saturating_add(reads)
                    .saturating_add(1)
            })
            .fold(1, usize::saturating_add),
    })
}

fn resolve_term(term: &Term, held: &[BTreeSet<Held>]) -> Result<Vec<Held>, CompilerFailure> {
    match &term.source {
        Source::Caller(root) => {
            let origin = Origin {
                root: root.clone(),
                steps: term.steps.clone(),
            };
            Ok(vec![Held::of(origin, term)])
        }
        Source::Local(LocalId(index)) => {
            let values = held.get(*index).ok_or_else(undeclared)?;
            Ok(values.iter().map(|value| value.through(term)).collect())
        }
    }
}

fn undeclared() -> CompilerFailure {
    crate::typechecker::invariant_failure("a local the walk never declared is used")
}

fn term_local(term: &Term) -> Option<LocalId> {
    match term.source {
        Source::Local(local) => Some(local),
        Source::Caller(_) => None,
    }
}

fn contains(outer: Span, inner: Span) -> bool {
    outer.file == inner.file && outer.start <= inner.start && inner.end <= outer.end
}

/// Whether `expr` names one of the runtime's own bindings, such as
/// `console`, whose methods keep nothing the body reads back.
fn is_runtime_global(expr: &TypedExpr) -> bool {
    matches!(&expr.kind, TypedExprKind::GlobalRef { mangled, .. } if is_runtime_symbol(mangled))
}

/// The values a flow names as they are, before any computation.
fn sources_read(flow: &Flow) -> BTreeSet<Source> {
    flow.iter()
        .filter(|term| !term.derived)
        .map(|term| term.source.clone())
        .collect()
}

fn read(flow: &Flow, key: &ReadKey) -> Flow {
    flow.iter().map(|term| term.read(key)).collect()
}

fn derived(flow: Flow) -> Flow {
    flow.into_iter()
        .map(|term| Term {
            derived: true,
            ..term
        })
        .collect()
}

/// The flow of a comparison with `null`, a truthiness test, or a `typeof` or
/// `instanceof` of each value `flow` holds.
fn identity(flow: Flow) -> Flow {
    using(flow, Reliance::Identity)
}

/// The flow of a computation that uses each value `flow` holds as `used`
/// does. A value already computed keeps what its own computation used.
fn using(flow: Flow, used: Reliance) -> Flow {
    flow.into_iter()
        .map(|term| Term {
            used: if term.derived { term.used } else { used },
            derived: true,
            ..term
        })
        .collect()
}

fn position(span: Span) -> (u32, u32, u32) {
    (span.file.0, span.start, span.end)
}
