//! Follows the caller's values through one body that calls `check()`.
//!
//! The walk is flow-insensitive: reads in exclusive branches count together.

use std::collections::{BTreeMap, BTreeSet};

use super::super::check_calls::SearchRoot;
use super::bodies::Body;
use super::findings::{Findings, ReadProblem, RootNote, Usage};
use super::origin::{CallerValue, Origin, ParameterId, ReadKey, ValueRoot};
use super::package::{PackageFacts, is_synthetic, parameter_shown, source_name};
use super::scope::Scopes;
use super::stability::is_primitive;
use crate::compiler_error::CompilerFailure;
use crate::typechecker::infer::narrowing::{BindingId, ReferencePath};
use crate::{
    BinOp, ClosureBody, ExprId, Ident, MangledName, PostfixTarget, Span, StmtId, Type,
    TypedArrayElement, TypedCatchClause, TypedChainPart, TypedExpr, TypedExprKind,
    TypedObjectMember, TypedParam, TypedStmtKind, UnOp,
};

pub(super) fn analyse(
    package: &PackageFacts<'_>,
    body: &Body<'_>,
    checking_closures: &BTreeSet<ExprId>,
    findings: &mut Findings<'_>,
) -> Result<(), CompilerFailure> {
    let mut walker = Walker {
        package,
        body,
        checking_closures,
        root_notes: BTreeMap::new(),
        scopes: Scopes::default(),
        closure_depth: 0,
        loop_depth: 0,
        reads: Vec::new(),
        evaluated: BTreeMap::new(),
        findings,
    };
    walker.body()
}

/// The caller's values an expression may yield. Empty for a stable value, and
/// more than one only out of a conditional.
type Yield = Vec<CallerValue>;

/// What is done with the value an expression yields.
enum Use {
    /// Compared, tested or dropped: nothing of the value is read.
    Inspect,
    Escape(Usage),
}

struct Read {
    origin: Origin,
    key: ReadKey,
    span: Span,
}

/// What the walk knows of a name in scope.
#[derive(Clone, Debug)]
struct Binding {
    /// `None` for a stable value.
    value: Option<CallerValue>,
    /// How many closures enclose the declaration.
    closure_depth: u32,
}

struct Walker<'a, 'f, 'l> {
    package: &'a PackageFacts<'a>,
    body: &'a Body<'a>,
    /// The closures with a `check()` directly in their own body.
    checking_closures: &'a BTreeSet<ExprId>,
    /// Why each [`ValueRoot::Parameter`] is the caller's.
    root_notes: BTreeMap<ParameterId, RootNote>,
    scopes: Scopes<Binding>,
    closure_depth: u32,
    loop_depth: u32,
    reads: Vec<Read>,
    /// A node two parents share is evaluated once at run time, as the receiver
    /// of a compound assignment is: its reads count once.
    evaluated: BTreeMap<ExprId, Yield>,
    findings: &'f mut Findings<'l>,
}

impl<'a> Walker<'a, '_, '_> {
    fn body(&mut self) -> Result<(), CompilerFailure> {
        let body = self.body;
        self.scopes.push();
        let owner = format!("`{}`", body.label);
        self.declare_params(body.params, None, &owner);
        self.store_parameter_properties();
        // A function expression can name itself, and the name holds the
        // package's own function.
        let own_name = body
            .function_value
            .and_then(|function| self.package.ta.closure_names.get(&function));
        if let Some(name) = own_name {
            self.declare(name, None);
        }
        let walked = match body.root {
            SearchRoot::Stmt(block) => self.stmt(block),
            SearchRoot::Expr(result) => self.consume(result, Use::Escape(Usage::Returned)),
        };
        self.scopes.pop();
        walked
    }

    fn stmt(&mut self, id: StmtId) -> Result<(), CompilerFailure> {
        let ta = self.package.ta;
        let stmt = ta.try_stmt(id).map_err(crate::typechecker::arena_failure)?;
        let _: () = match &stmt.kind {
            TypedStmtKind::Let { name, value, .. } | TypedStmtKind::Const { name, value, .. } => {
                self.bind(name, *value)?;
            }
            TypedStmtKind::Expr(value) => self.consume(*value, Use::Inspect)?,
            TypedStmtKind::Throw { value } => self.consume(*value, Use::Escape(Usage::Thrown))?,
            TypedStmtKind::Return(value) => {
                if let Some(value) = value {
                    self.consume(*value, Use::Escape(Usage::Returned))?;
                }
            }
            TypedStmtKind::AssignLocal { ident, value, .. } => self.assign_local(ident, *value)?,
            TypedStmtKind::AssignGlobal { ident, value, .. } => {
                let target = format!("`{}`", ident.name);
                self.consume(*value, Use::Escape(Usage::Stored(target)))?;
            }
            TypedStmtKind::AssignField {
                receiver,
                name,
                value,
            } => {
                self.write_through(*receiver)?;
                let target = format!("the property `{}`", name.name);
                self.consume(*value, Use::Escape(Usage::Stored(target)))?;
            }
            TypedStmtKind::AssignIndex {
                receiver,
                index,
                value,
                ..
            } => {
                self.write_through(*receiver)?;
                self.consume(*index, Use::Escape(Usage::Operand))?;
                let target = "an element".to_string();
                self.consume(*value, Use::Escape(Usage::Stored(target)))?;
            }
            TypedStmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                self.consume(*condition, Use::Inspect)?;
                self.scoped(*then_block)?;
                if let Some(else_block) = else_block {
                    self.scoped(*else_block)?;
                }
            }
            TypedStmtKind::While { condition, body }
            | TypedStmtKind::DoWhile { body, condition } => {
                self.in_loop(|walker| {
                    walker.consume(*condition, Use::Inspect)?;
                    walker.scoped(*body)
                })?;
            }
            TypedStmtKind::For {
                init,
                condition,
                update,
                body,
            } => {
                self.scopes.push();
                let walked = self.for_loop(*init, *condition, *update, *body);
                self.scopes.pop();
                walked?;
            }
            TypedStmtKind::ForOf {
                name,
                element_ty,
                iter,
                body,
                ..
            } => self.for_of(name, element_ty, *iter, *body)?,
            TypedStmtKind::Switch {
                discriminant,
                cases,
                default,
                ..
            } => {
                self.consume(*discriminant, Use::Inspect)?;
                for case in cases {
                    self.scoped(case.body)?;
                }
                if let Some(default) = default {
                    self.scoped(*default)?;
                }
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
            // use, so walking it would count a read that never runs.
            TypedStmtKind::NarrowRegion { body, .. } => self.stmt(*body)?,
            TypedStmtKind::Break | TypedStmtKind::Continue | TypedStmtKind::ReboxLocal { .. } => {}
        };
        Ok(())
    }

    fn scoped(&mut self, id: StmtId) -> Result<(), CompilerFailure> {
        self.scopes.push();
        let walked = self.stmt(id);
        self.scopes.pop();
        walked
    }

    fn in_loop(
        &mut self,
        walk: impl FnOnce(&mut Self) -> Result<(), CompilerFailure>,
    ) -> Result<(), CompilerFailure> {
        self.loop_depth = self.loop_depth.saturating_add(1);
        let walked = walk(self);
        self.loop_depth = self.loop_depth.saturating_sub(1);
        walked
    }

    fn for_loop(
        &mut self,
        init: Option<StmtId>,
        condition: Option<ExprId>,
        update: Option<StmtId>,
        body: StmtId,
    ) -> Result<(), CompilerFailure> {
        if let Some(init) = init {
            self.stmt(init)?;
        }
        self.in_loop(|walker| {
            if let Some(condition) = condition {
                walker.consume(condition, Use::Inspect)?;
            }
            if let Some(update) = update {
                walker.stmt(update)?;
            }
            walker.scoped(body)
        })
    }

    fn for_of(
        &mut self,
        name: &Ident,
        element_ty: &Type,
        iter: ExprId,
        body: StmtId,
    ) -> Result<(), CompilerFailure> {
        let span = self.expr_at(iter)?.span;
        let iterated = self.eval(iter)?;
        let yields_primitive = is_primitive(element_ty);
        self.in_loop(|walker| {
            // The element exists in the loop that reads it, one for each pass.
            let element = walker.single(iterated, span).and_then(|iterated| {
                walker.read_at(&iterated, ReadKey::Iteration, span, yields_primitive, 1)
            });
            walker.scopes.push();
            walker.declare(name, element);
            let walked = walker.stmt(body);
            walker.scopes.pop();
            walked
        })
    }

    fn catch(&mut self, clause: &TypedCatchClause) -> Result<(), CompilerFailure> {
        self.scopes.push();
        self.declare(&clause.binding, None);
        let walked = self.stmt(clause.body);
        self.scopes.pop();
        walked
    }

    fn bind(&mut self, name: &Ident, value: ExprId) -> Result<(), CompilerFailure> {
        let span = self.expr_at(value)?.span;
        let yielded = self.eval(value)?;
        let value = self.single(yielded, span);
        self.declare(name, value);
        Ok(())
    }

    /// A value that came to exist at this depth is bound by this declaration;
    /// an older one keeps the binding it has outside the loop.
    fn declare(&mut self, name: &Ident, value: Option<CallerValue>) {
        let value = value.map(|value| CallerValue {
            bound: if value.loop_depth == self.loop_depth {
                name.span
            } else {
                value.bound
            },
            ..value
        });
        self.scopes.declare(
            &name.name,
            Binding {
                value,
                closure_depth: self.closure_depth,
            },
        );
    }

    /// Assigning is reported rather than followed: the binding's earlier
    /// uses, which a loop can run again, were judged on its earlier value.
    fn assign_local(&mut self, ident: &Ident, value: ExprId) -> Result<(), CompilerFailure> {
        let span = self.expr_at(value)?.span;
        let yielded = self.eval(value)?;
        let Some(assigned) = self.single(yielded, span) else {
            return Ok(());
        };
        let held = self
            .scopes
            .resolve(&ident.name)
            .and_then(|binding| binding.value.as_ref());
        if held.is_some_and(|held| held.origin == assigned.origin) {
            return Ok(());
        }
        let target = format!("`{}`", ident.name);
        self.escape(&assigned, Usage::Stored(target), span);
        Ok(())
    }

    fn write_through(&mut self, receiver: ExprId) -> Result<(), CompilerFailure> {
        let span = self.expr_at(receiver)?.span;
        let yielded = self.eval(receiver)?;
        if let Some(receiver) = self.single(yielded, span)
            && !receiver.origin.is_bare_this()
        {
            self.escape(&receiver, Usage::Written, span);
        }
        Ok(())
    }

    /// Declares the parameters of the body, or of `function` when it is a
    /// nested function that calls `check()`: the caller supplies both.
    fn declare_params(&mut self, params: &[TypedParam], function: Option<ExprId>, owner: &str) {
        for (position, param) in params.iter().enumerate() {
            let parameter = ParameterId { function, position };
            let value =
                (!is_primitive(&param.ty)).then(|| self.parameter_root(param, parameter, owner));
            self.declare(&param.name, value);
        }
    }

    fn parameter_root(
        &mut self,
        param: &TypedParam,
        parameter: ParameterId,
        owner: &str,
    ) -> CallerValue {
        let shown = parameter_shown(param, parameter.position);
        self.root_notes.insert(
            parameter,
            RootNote {
                span: param.name.span,
                message: format!("`{shown}` is a parameter of {owner}"),
            },
        );
        CallerValue {
            origin: Origin::of(ValueRoot::Parameter(parameter)),
            shown,
            loop_depth: self.loop_depth,
            bound: param.name.span,
        }
    }

    /// A parameter property is stored without an assignment to walk.
    fn store_parameter_properties(&mut self) {
        let body = self.body;
        for field in &body.parameter_properties {
            let Some(value) = self
                .scopes
                .resolve(&field.name.name)
                .and_then(|binding| binding.value.clone())
            else {
                continue;
            };
            let target = format!("the property `{}`", field.name.name);
            self.escape(&value, Usage::Stored(target), field.name.span);
        }
    }

    fn consume(&mut self, id: ExprId, use_: Use) -> Result<(), CompilerFailure> {
        let span = self.expr_at(id)?.span;
        let yielded = self.eval(id)?;
        match use_ {
            Use::Inspect => {}
            Use::Escape(usage) => {
                if let Some(value) = self.single(yielded, span) {
                    self.escape(&value, usage, span);
                }
            }
        }
        Ok(())
    }

    fn consume_all(
        &mut self,
        ids: &[ExprId],
        usage: impl Fn() -> Usage,
    ) -> Result<(), CompilerFailure> {
        ids.iter()
            .try_for_each(|id| self.consume(*id, Use::Escape(usage())))
    }

    fn eval(&mut self, id: ExprId) -> Result<Yield, CompilerFailure> {
        if let Some(yielded) = self.evaluated.get(&id) {
            return Ok(yielded.clone());
        }
        let expr = self.expr_at(id)?;
        let mut yielded = self.eval_kind(id, expr)?;
        if is_primitive(&expr.ty) {
            yielded.clear();
        }
        self.evaluated.insert(id, yielded.clone());
        Ok(yielded)
    }

    fn eval_kind(&mut self, id: ExprId, expr: &'a TypedExpr) -> Result<Yield, CompilerFailure> {
        let span = expr.span;
        let yields_primitive = is_primitive(&expr.ty);
        Ok(match &expr.kind {
            TypedExprKind::Number(_)
            | TypedExprKind::BigInt(_)
            | TypedExprKind::String(_)
            | TypedExprKind::Boolean(_)
            | TypedExprKind::Null
            | TypedExprKind::Regex { .. }
            | TypedExprKind::FunctionRef { .. }
            | TypedExprKind::NumberEnumMember { .. }
            | TypedExprKind::StringEnumMember { .. } => stable(),
            TypedExprKind::This => self.this(span, yields_primitive),
            TypedExprKind::LocalRef { ident, .. } => {
                self.local(&ident.name, span, yields_primitive)
            }
            TypedExprKind::GlobalRef { mangled, name } => {
                self.global(mangled, &name.name, span, yields_primitive)
            }
            TypedExprKind::LocalNarrowRef { path, .. } => {
                self.narrowed_reference(path, span, yields_primitive)
            }
            TypedExprKind::Call { mangled, args, .. } => {
                self.call(mangled, span, args)?;
                stable()
            }
            TypedExprKind::GenericCall { mangled, args, .. } => {
                let args: Vec<ExprId> = args.iter().map(|arg| arg.expr).collect();
                self.call(mangled, span, &args)?;
                stable()
            }
            TypedExprKind::McpCall { tool, args, .. } => {
                let args = self.package.authored(span, args);
                self.consume_all(args, || Usage::PassedTo(tool.clone()))?;
                stable()
            }
            TypedExprKind::SuperCtorCall { args, .. } => {
                let args = self.package.authored(span, args);
                self.consume_all(args, || Usage::PassedTo("super".to_string()))?;
                stable()
            }
            TypedExprKind::SuperMethodCall { name, args, .. } => {
                let args = self.package.authored(span, args);
                self.consume_all(args, || Usage::PassedTo(format!("super.{}", name.name)))?;
                stable()
            }
            TypedExprKind::IntrinsicCall { kind, args } => {
                let args = self.package.authored(span, args);
                self.consume_all(args, || Usage::PassedTo(kind.name().to_string()))?;
                stable()
            }
            TypedExprKind::CallClosure { callee, args } => {
                self.consume(*callee, Use::Escape(Usage::Called))?;
                let args = self.package.authored(span, args);
                self.consume_all(args, || Usage::PassedToFunctionValue)?;
                stable()
            }
            TypedExprKind::MethodCall {
                receiver,
                name,
                args,
                ..
            } => {
                self.method_call(*receiver, name, self.package.authored(span, args))?;
                stable()
            }
            TypedExprKind::GenericMethodCall {
                receiver,
                name,
                args,
                ..
            } => {
                let args: Vec<ExprId> = args.iter().map(|arg| arg.expr).collect();
                self.method_call(*receiver, name, self.package.authored(span, &args))?;
                stable()
            }
            TypedExprKind::Binary { op, lhs, rhs } => self.binary(*op, *lhs, *rhs)?,
            TypedExprKind::NullishCoalesce { lhs, rhs } => self.either(*lhs, *rhs)?,
            TypedExprKind::Ternary { cond, then_, else_ } => {
                self.consume(*cond, Use::Inspect)?;
                self.either(*then_, *else_)?
            }
            TypedExprKind::Unary { op, operand } => {
                let use_ = match op {
                    UnOp::Not => Use::Inspect,
                    UnOp::Neg | UnOp::Pos => Use::Escape(Usage::Operand),
                };
                self.consume(*operand, use_)?;
                stable()
            }
            TypedExprKind::TypeofTag { value, .. } | TypedExprKind::InstanceOf { value, .. } => {
                self.consume(*value, Use::Inspect)?;
                stable()
            }
            TypedExprKind::EffectThen { effect, result } => {
                self.consume(*effect, Use::Inspect)?;
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
                let key = ReadKey::Property(name.name.clone());
                self.read_of(*receiver, key, span, yields_primitive)?
            }
            TypedExprKind::IndexAccess { receiver, index } => {
                let key = self.package.index_key(*index)?;
                self.consume(*index, Use::Escape(Usage::Operand))?;
                self.read_of(*receiver, key, span, yields_primitive)?
            }
            TypedExprKind::OptionalChain { base, parts } => {
                self.optional_chain(*base, parts, span)?
            }
            TypedExprKind::ObjectLiteral { members, .. } => {
                self.object_literal(members, || Usage::Stored("an object literal".to_string()))?;
                stable()
            }
            TypedExprKind::ArrayLiteral { elements, .. } => {
                for element in elements {
                    let usage = match element {
                        TypedArrayElement::Value(_) => {
                            Usage::Stored("an array literal".to_string())
                        }
                        TypedArrayElement::Spread(_) => Usage::Spread,
                    };
                    self.consume(element.expr_id(), Use::Escape(usage))?;
                }
                stable()
            }
            TypedExprKind::TupleLiteral { elements, .. } => {
                self.consume_all(elements, || Usage::Stored("an array literal".to_string()))?;
                stable()
            }
            TypedExprKind::Closure { params, body, .. } => {
                self.closure(id, params, body)?;
                stable()
            }
            TypedExprKind::PostfixUnary { target, .. } => {
                self.postfix(target, span)?;
                stable()
            }
        })
    }

    fn this(&mut self, span: Span, yields_primitive: bool) -> Yield {
        if yields_primitive || !self.body.has_receiver {
            return stable();
        }
        let value = CallerValue {
            origin: Origin::of(ValueRoot::This),
            shown: "this".to_string(),
            loop_depth: 0,
            bound: self.body.label_span,
        };
        self.unless_captured(value, 0, span)
    }

    fn local(&mut self, name: &str, span: Span, yields_primitive: bool) -> Yield {
        if yields_primitive {
            return stable();
        }
        let Some(binding) = self.scopes.resolve(name) else {
            return vec![CallerValue {
                origin: Origin::of(ValueRoot::Unresolved(name.to_string())),
                shown: name.to_string(),
                loop_depth: 0,
                bound: span,
            }];
        };
        let Some(value) = binding.value.clone() else {
            return stable();
        };
        let declared_at = binding.closure_depth;
        let value = if is_synthetic(name) {
            value
        } else {
            CallerValue {
                shown: name.to_string(),
                ..value
            }
        };
        self.unless_captured(value, declared_at, span)
    }

    fn global(
        &mut self,
        mangled: &MangledName,
        name: &str,
        span: Span,
        yields_primitive: bool,
    ) -> Yield {
        if yields_primitive {
            return stable();
        }
        let Some(bound) = self.package.caller_global(mangled) else {
            return stable();
        };
        let value = CallerValue {
            origin: Origin::of(ValueRoot::Global(mangled.clone())),
            shown: name.to_string(),
            loop_depth: 0,
            bound: bound.unwrap_or(span),
        };
        self.unless_captured(value, 0, span)
    }

    /// A reference from a nested function runs when the function does, which
    /// may be later and more than once.
    fn unless_captured(&mut self, value: CallerValue, declared_at: u32, span: Span) -> Yield {
        if declared_at < self.closure_depth {
            self.escape(&value, Usage::Captured, span);
            return stable();
        }
        vec![value]
    }

    /// A narrowed reference reads its path again at every use.
    fn narrowed_reference(
        &mut self,
        path: &ReferencePath,
        span: Span,
        yields_primitive: bool,
    ) -> Yield {
        let root = match &path.root {
            BindingId::Local { name, .. } => self.local(name, span, false),
            BindingId::Global(mangled) => self.global(mangled, source_name(mangled), span, false),
            BindingId::This => self.this(span, false),
        };
        let mut current = self.single(root, span);
        let mut elements = path.chain.iter().peekable();
        while let Some(element) = elements.next() {
            let Some(value) = current else {
                return stable();
            };
            let key = ReadKey::of_path(element);
            let last = elements.peek().is_none();
            current = self.read(&value, key, span, last && yields_primitive);
        }
        current.into_iter().collect()
    }

    fn call(
        &mut self,
        mangled: &MangledName,
        span: Span,
        args: &[ExprId],
    ) -> Result<(), CompilerFailure> {
        let args = self.package.authored(span, args);
        if crate::stdlib::security::is_check(mangled) {
            return args.iter().try_for_each(|arg| self.check_argument(*arg));
        }
        let callee = source_name(mangled);
        self.consume_all(args, || Usage::PassedTo(callee.to_string()))
    }

    /// The context is usually a literal, whose members `check()` reads.
    fn check_argument(&mut self, arg: ExprId) -> Result<(), CompilerFailure> {
        match &self.expr_at(arg)?.kind {
            TypedExprKind::ObjectLiteral { members, .. } => {
                self.evaluated.insert(arg, stable());
                self.object_literal(members, || Usage::CheckContext)
            }
            _ => self.consume(arg, Use::Escape(Usage::CheckContext)),
        }
    }

    fn method_call(
        &mut self,
        receiver: ExprId,
        name: &Ident,
        args: &[ExprId],
    ) -> Result<(), CompilerFailure> {
        self.consume(
            receiver,
            Use::Escape(Usage::MethodCalled(name.name.clone())),
        )?;
        self.consume_all(args, || Usage::PassedTo(name.name.clone()))
    }

    fn binary(&mut self, op: BinOp, lhs: ExprId, rhs: ExprId) -> Result<Yield, CompilerFailure> {
        let use_ = || match op {
            BinOp::Eq
            | BinOp::NotEq
            | BinOp::Lt
            | BinOp::Gt
            | BinOp::Le
            | BinOp::Ge
            | BinOp::In => Some(Use::Inspect),
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem | BinOp::Pow => {
                Some(Use::Escape(Usage::Operand))
            }
            BinOp::And | BinOp::Or | BinOp::NullishCoalesce => None,
        };
        let (Some(left), Some(right)) = (use_(), use_()) else {
            return self.either(lhs, rhs);
        };
        self.consume(lhs, left)?;
        self.consume(rhs, right)?;
        Ok(stable())
    }

    /// A conditional yields what either operand does, to whatever uses it.
    fn either(&mut self, first: ExprId, second: ExprId) -> Result<Yield, CompilerFailure> {
        let mut yielded = self.eval(first)?;
        for value in self.eval(second)? {
            if !yielded.iter().any(|other| other.origin == value.origin) {
                yielded.push(value);
            }
        }
        Ok(yielded)
    }

    fn read_of(
        &mut self,
        receiver: ExprId,
        key: ReadKey,
        span: Span,
        yields_primitive: bool,
    ) -> Result<Yield, CompilerFailure> {
        let receiver_span = self.expr_at(receiver)?.span;
        let yielded = self.eval(receiver)?;
        Ok(self
            .single(yielded, receiver_span)
            .and_then(|receiver| self.read(&receiver, key, span, yields_primitive))
            .into_iter()
            .collect())
    }

    fn optional_chain(
        &mut self,
        base: ExprId,
        parts: &[TypedChainPart],
        span: Span,
    ) -> Result<Yield, CompilerFailure> {
        let base_span = self.expr_at(base)?.span;
        let yielded = self.eval(base)?;
        let mut current = self.single(yielded, base_span);
        for part in parts {
            current = self.chain_part(current, part, span)?;
        }
        Ok(current.into_iter().collect())
    }

    fn chain_part(
        &mut self,
        receiver: Option<CallerValue>,
        part: &TypedChainPart,
        chain: Span,
    ) -> Result<Option<CallerValue>, CompilerFailure> {
        let yields_primitive = is_primitive(part.result_ty());
        let read = |walker: &mut Self, key: ReadKey, span: Span| {
            receiver
                .as_ref()
                .and_then(|receiver| walker.read(receiver, key, span, yields_primitive))
        };
        Ok(match part {
            TypedChainPart::Field { name, span, .. }
            | TypedChainPart::InterfaceProperty { name, span, .. } => {
                read(self, ReadKey::Property(name.name.clone()), *span)
            }
            TypedChainPart::Index { idx, span, .. } => {
                let key = self.package.index_key(*idx)?;
                self.consume(*idx, Use::Escape(Usage::Operand))?;
                read(self, key, *span)
            }
            TypedChainPart::Call { args, span, .. } => {
                if let Some(receiver) = &receiver {
                    self.escape(receiver, Usage::Called, chain);
                }
                let args = self.package.authored(*span, args);
                self.consume_all(args, || Usage::PassedToFunctionValue)?;
                None
            }
            TypedChainPart::MethodCall {
                name, args, span, ..
            } => {
                if let Some(receiver) = &receiver {
                    self.escape(receiver, Usage::MethodCalled(name.name.clone()), chain);
                }
                let args = self.package.authored(*span, args);
                self.consume_all(args, || Usage::PassedTo(name.name.clone()))?;
                None
            }
            TypedChainPart::NonNull { .. } => receiver,
        })
    }

    fn object_literal(
        &mut self,
        members: &[TypedObjectMember],
        stored: impl Fn() -> Usage,
    ) -> Result<(), CompilerFailure> {
        for member in members {
            let _: () = match member {
                TypedObjectMember::Value(value) => self.consume(*value, Use::Escape(stored()))?,
                TypedObjectMember::Computed { key, value } => {
                    self.consume(*key, Use::Escape(Usage::Operand))?;
                    self.consume(*value, Use::Escape(stored()))?;
                }
                TypedObjectMember::Spread { source, .. } => {
                    self.consume(*source, Use::Escape(Usage::Spread))?;
                }
            };
        }
        Ok(())
    }

    /// A nested function's parameters are whatever its caller passes, which is
    /// the package's own code unless the function stands in for a body by
    /// calling `check()` itself.
    fn closure(
        &mut self,
        id: ExprId,
        params: &[TypedParam],
        body: &ClosureBody,
    ) -> Result<(), CompilerFailure> {
        let outer_loop_depth = std::mem::replace(&mut self.loop_depth, 0);
        self.closure_depth = self.closure_depth.saturating_add(1);
        self.scopes.push();
        if let Some(name) = self.package.ta.closure_names.get(&id) {
            self.declare(name, None);
        }
        if self.checking_closures.contains(&id) {
            self.declare_params(params, Some(id), "a nested function that calls `check()`");
        } else {
            for param in params {
                self.declare(&param.name, None);
            }
        }
        let walked = match body {
            ClosureBody::Expr(value) => self.consume(*value, Use::Escape(Usage::Returned)),
            ClosureBody::Block(block) => self.stmt(*block),
        };
        self.scopes.pop();
        self.closure_depth = self.closure_depth.saturating_sub(1);
        self.loop_depth = outer_loop_depth;
        walked
    }

    fn postfix(&mut self, target: &PostfixTarget, span: Span) -> Result<(), CompilerFailure> {
        let (receiver, key) = match target {
            PostfixTarget::Local { .. } | PostfixTarget::Global { .. } => return Ok(()),
            PostfixTarget::Field { receiver, name, .. } => {
                (*receiver, ReadKey::Property(name.name.clone()))
            }
            PostfixTarget::Index {
                receiver, index, ..
            } => {
                let key = self.package.index_key(*index)?;
                self.consume(*index, Use::Escape(Usage::Operand))?;
                (*receiver, key)
            }
        };
        self.write_through(receiver)?;
        self.read_of(receiver, key, span, true)?;
        Ok(())
    }

    /// The one value `yielded` holds. More than one is reported, and the
    /// result treated as stable, so that the merge is the only warning.
    fn single(&mut self, yielded: Yield, span: Span) -> Option<CallerValue> {
        if yielded.len() > 1 {
            self.findings.report_merged_values(&yielded, span);
            return None;
        }
        yielded.into_iter().next()
    }

    fn read(
        &mut self,
        receiver: &CallerValue,
        key: ReadKey,
        span: Span,
        yields_primitive: bool,
    ) -> Option<CallerValue> {
        self.read_at(receiver, key, span, yields_primitive, 0)
    }

    /// Reads `key` of `receiver`. `deeper` is how many loops further in than
    /// the read what it yields comes to exist, which is one for the element a
    /// loop iterates over.
    fn read_at(
        &mut self,
        receiver: &CallerValue,
        key: ReadKey,
        span: Span,
        yields_primitive: bool,
        deeper: u32,
    ) -> Option<CallerValue> {
        let read_depth = self.loop_depth.saturating_sub(deeper);
        if let Some(problem) = self.problem_with(receiver, &key, read_depth) {
            self.findings.report_read(problem, receiver, &key, span);
            return None;
        }
        self.reads.push(Read {
            origin: receiver.origin.clone(),
            key: key.clone(),
            span,
        });
        if yields_primitive {
            return None;
        }
        Some(receiver.after(&key, self.loop_depth))
    }

    fn problem_with(
        &self,
        receiver: &CallerValue,
        key: &ReadKey,
        read_depth: u32,
    ) -> Option<ReadProblem> {
        if read_depth > receiver.loop_depth {
            return Some(ReadProblem::InLoop);
        }
        let earlier = self.reads.iter().find(|earlier| {
            earlier.origin == receiver.origin && key.conflicts_with(&earlier.key)
        })?;
        let first = earlier.span;
        Some(if key.reads_elements() {
            ReadProblem::ElementsRepeated { first }
        } else {
            ReadProblem::Repeated { first }
        })
    }

    fn escape(&mut self, value: &CallerValue, usage: Usage, span: Span) {
        let why = self.root_note_for(value, span);
        self.findings.report_usage(value, usage, span, why);
    }

    fn root_note_for(&self, value: &CallerValue, used: Span) -> RootNote {
        let label = &self.body.label;
        match &value.origin.root {
            ValueRoot::Parameter(parameter) => {
                self.root_notes.get(parameter).cloned().unwrap_or(RootNote {
                    span: value.bound,
                    message: format!("`{}` is a parameter of `{label}`", value.shown),
                })
            }
            ValueRoot::This => RootNote {
                span: self.body.label_span,
                message: format!("`this` is the instance the caller runs `{label}` on"),
            },
            ValueRoot::Global(mangled) => RootNote {
                span: value.bound,
                message: format!("the caller can reach the global `{}`", source_name(mangled)),
            },
            ValueRoot::Unresolved(name) => RootNote {
                span: used,
                message: format!("`{name}` has no declaration in `{label}`"),
            },
        }
    }

    fn expr_at(&self, id: ExprId) -> Result<&'a TypedExpr, CompilerFailure> {
        self.package.expr_at(id)
    }
}

fn stable() -> Yield {
    Vec::new()
}
