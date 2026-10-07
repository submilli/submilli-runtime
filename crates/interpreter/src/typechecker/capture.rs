//! Capture analysis pass — marks `let` bindings, parameters, and their
//! references as boxed when they're captured by inner closures, and
//! populates each `Closure.captured` with the env-layout codegen needs.

use std::collections::{BTreeMap, BTreeSet};

use crate::compiler_error::{CompilerFailure, CompilerStage};
use crate::{
    CapturedVar, ClosureBody, ExprId, Ident, StmtId, Type, TypedAst, TypedChainPart, TypedExprKind,
    TypedStmtKind, tree_height,
};

/// Capture-pass name for `this`. `this` is a keyword, so it can never collide
/// with a user binding, and codegen recognises the same key when it rebinds the
/// receiver inside a closure body.
pub(crate) const THIS_BINDING: &str = "this";

pub fn capture(mut ta: TypedAst) -> Result<TypedAst, CompilerFailure> {
    tree_height::check_typed(&ta, CompilerStage::Infer)?;
    resolve_locals(&mut ta)?;
    Ok(ta)
}

/// Declaration identities for storage-flow analysis after desugaring. The
/// declaration span distinguishes same-named bindings in nested scopes.
#[derive(Default)]
pub(crate) struct ResolvedLocals {
    pub reads: BTreeMap<ExprId, Ident>,
    pub writes: BTreeMap<StmtId, Ident>,
}

pub(crate) fn resolve_locals(
    ta: &mut TypedAst,
) -> Result<ResolvedLocals, crate::compiler_error::CompilerFailure> {
    let mut state = State {
        ta,
        frames: Vec::new(),
        pending: Vec::new(),
        resolved: ResolvedLocals::default(),
    };
    let function_bodies: Vec<(usize, Vec<crate::TypedParam>, StmtId)> = state
        .ta
        .functions
        .iter()
        .enumerate()
        .map(|(idx, f)| (idx, f.params.clone(), f.body))
        .collect();
    for (idx, params, body) in function_bodies {
        state.walk_function(idx, &params, body)?;
    }
    state.frames.push(Frame::default());
    let stmt_ids: Vec<StmtId> = state.ta.top_level_statements.clone();
    for sid in stmt_ids {
        state.walk_stmt(sid)?;
    }
    state.pop_frame()?;
    // Class member bodies are function scopes too: a closure inside a method
    // captures that method's params and locals, and without this pass its
    // capture list stays empty and codegen has no local to read.
    for m in class_member_bodies(state.ta) {
        state.walk_class_member(m.member, &m.params, m.body, m.this_ty)?;
    }
    for (this_ty, expr_id) in field_initializers_with_receiver(state.ta) {
        state.walk_initializer(expr_id, this_ty)?;
    }
    // Deferred so refs before a capturing closure still see the final `boxed` flag.
    state.apply_pending()?;
    Ok(state.resolved)
}

/// A class member body, the parameters in scope for it, and the instance type
/// of `this` inside it.
struct MemberBody {
    member: ClassMemberRef,
    params: Vec<crate::TypedParam>,
    body: StmtId,
    this_ty: Type,
}

/// Field-initializer expressions paired with the `this` in scope for them.
/// They run inside the constructor, so `this` is available exactly as it is in
/// a member body. Distinct from `TypedAst::class_field_initializers`, which
/// yields the bare expressions for passes that need no receiver.
fn field_initializers_with_receiver(ta: &TypedAst) -> Vec<(Type, ExprId)> {
    let mut out = Vec::new();
    for ty_decl in &ta.types {
        let crate::TypedTypeDecl::Class(c) = ty_decl else {
            continue;
        };
        let this_ty = Type::class_ref(
            crate::Package(ta.package_name.clone()),
            c.name.name.clone(),
            c.mangled_name.clone(),
            Vec::new(),
        );
        for f in &c.fields {
            if let Some(init) = f.initializer {
                out.push((this_ty.clone(), init));
            }
        }
    }
    out
}

fn class_member_bodies(ta: &TypedAst) -> Vec<MemberBody> {
    let mut out = Vec::new();
    for (decl, ty_decl) in ta.types.iter().enumerate() {
        let crate::TypedTypeDecl::Class(c) = ty_decl else {
            continue;
        };
        let this_ty = Type::class_ref(
            crate::Package(ta.package_name.clone()),
            c.name.name.clone(),
            c.mangled_name.clone(),
            Vec::new(),
        );
        if let Some(ctor) = &c.constructor {
            out.push(MemberBody {
                member: ClassMemberRef::Constructor { decl },
                params: ctor.params.clone(),
                body: ctor.body,
                this_ty: this_ty.clone(),
            });
        }
        for (index, m) in c.methods.iter().enumerate() {
            out.push(MemberBody {
                member: ClassMemberRef::Method { decl, index },
                params: m.params.clone(),
                body: m.body,
                this_ty: this_ty.clone(),
            });
        }
        for (index, a) in c.accessors.iter().enumerate() {
            let params = match a {
                crate::TypedClassAccessor::Getter { .. } => Vec::new(),
                crate::TypedClassAccessor::Setter { param, .. } => vec![param.clone()],
            };
            out.push(MemberBody {
                member: ClassMemberRef::Accessor { decl, index },
                params,
                body: a.body(),
                this_ty: this_ty.clone(),
            });
        }
    }
    out
}

struct State<'a> {
    ta: &'a mut TypedAst,
    frames: Vec<Frame>,
    pending: Vec<Pending>,
    resolved: ResolvedLocals,
}

enum Pending {
    LocalRef(ExprId, BindingSource),
    AssignLocal(StmtId, BindingSource),
    PostfixLocal(ExprId, BindingSource),
}

#[derive(Default)]
struct Frame {
    locals: BTreeMap<String, LocalBinding>,
    captures: Vec<CapturedVar>,
    captured_names: BTreeSet<String>,
}

#[derive(Clone)]
struct LocalBinding {
    name_ident: Ident,
    ty: Type,
    /// Captured `let`/params get boxed; captured `const`s are copied.
    mutable: bool,
    source: BindingSource,
}

#[derive(Clone)]
enum BindingSource {
    Let(StmtId),
    /// The binding of `catches[index]` in the `try` statement `try_stmt`.
    Catch {
        try_stmt: StmtId,
        index: usize,
    },
    Const,
    Param {
        owner: ParamOwner,
        index: usize,
    },
}

#[derive(Clone)]
enum ParamOwner {
    Function(usize),
    Closure(ExprId),
    ClassMember(ClassMemberRef),
}

/// Locates a class member's parameter list in `TypedAst::types`, so a captured
/// parameter can be marked boxed the same way a function's is.
#[derive(Clone, Copy)]
enum ClassMemberRef {
    Constructor { decl: usize },
    Method { decl: usize, index: usize },
    Accessor { decl: usize, index: usize },
}

impl ClassMemberRef {
    fn decl(&self) -> usize {
        let (Self::Constructor { decl } | Self::Method { decl, .. } | Self::Accessor { decl, .. }) =
            self;
        *decl
    }

    /// The member's parameter at `index`, wherever it lives. A setter has
    /// exactly one parameter and no `Vec` to index, so it answers for index 0.
    fn param_mut<'a>(
        &self,
        ta: &'a mut TypedAst,
        index: usize,
    ) -> Option<&'a mut crate::TypedParam> {
        let crate::TypedTypeDecl::Class(c) = ta.types.get_mut(self.decl())? else {
            return None;
        };
        match self {
            Self::Constructor { .. } => c.constructor.as_mut()?.params.get_mut(index),
            Self::Method { index: member, .. } => c.methods.get_mut(*member)?.params.get_mut(index),
            Self::Accessor { index: member, .. } => match c.accessors.get_mut(*member)? {
                crate::TypedClassAccessor::Setter { param, .. } if index == 0 => Some(param),
                _ => None,
            },
        }
    }

    fn param<'a>(&self, ta: &'a TypedAst, index: usize) -> Option<&'a crate::TypedParam> {
        let crate::TypedTypeDecl::Class(c) = ta.types.get(self.decl())? else {
            return None;
        };
        match self {
            Self::Constructor { .. } => c.constructor.as_ref()?.params.get(index),
            Self::Method { index: member, .. } => c.methods.get(*member)?.params.get(index),
            Self::Accessor { index: member, .. } => match c.accessors.get(*member)? {
                crate::TypedClassAccessor::Setter { param, .. } if index == 0 => Some(param),
                _ => None,
            },
        }
    }
}

impl State<'_> {
    fn walk_function(
        &mut self,
        function_idx: usize,
        params: &[crate::TypedParam],
        body: StmtId,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        self.walk_body(ParamOwner::Function(function_idx), params, body, None)?;
        Ok(())
    }

    fn walk_class_member(
        &mut self,
        member: ClassMemberRef,
        params: &[crate::TypedParam],
        body: StmtId,
        this_ty: Type,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        self.walk_body(ParamOwner::ClassMember(member), params, body, Some(this_ty))?;
        Ok(())
    }

    /// A field initializer is an expression root rather than a body, but it
    /// runs with the same `this` in scope.
    fn walk_initializer(
        &mut self,
        expr_id: ExprId,
        this_ty: Type,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        self.frames.push(Frame::default());
        self.bind(LocalBinding {
            name_ident: Ident {
                name: THIS_BINDING.to_string(),
                span: self
                    .ta
                    .try_expr(expr_id)
                    .map_err(crate::typechecker::arena_failure)?
                    .span,
            },
            ty: this_ty,
            mutable: false,
            source: BindingSource::Const,
        })?;
        self.walk_expr(expr_id)?;
        self.pop_frame()?;
        Ok(())
    }

    fn walk_body(
        &mut self,
        owner: ParamOwner,
        params: &[crate::TypedParam],
        body: StmtId,
        this_ty: Option<Type>,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        self.frames.push(Frame::default());
        // `this` is an ordinary immutable binding for capture purposes, so a
        // closure in a member body captures it by value like any `const`.
        if let Some(ty) = this_ty {
            self.bind(LocalBinding {
                name_ident: Ident {
                    name: THIS_BINDING.to_string(),
                    span: self
                        .ta
                        .try_stmt(body)
                        .map_err(crate::typechecker::arena_failure)?
                        .span,
                },
                ty,
                mutable: false,
                source: BindingSource::Const,
            })?;
        }
        for (i, p) in params.iter().enumerate() {
            self.bind(LocalBinding {
                name_ident: p.name.clone(),
                ty: p.ty.clone(),
                mutable: true,
                source: BindingSource::Param {
                    owner: owner.clone(),
                    index: i,
                },
            })?;
        }
        self.walk_stmt(body)?;
        self.pop_frame()?;
        Ok(())
    }

    fn walk_stmt(&mut self, id: StmtId) -> Result<(), crate::compiler_error::CompilerFailure> {
        let kind = self
            .ta
            .try_stmt(id)
            .map_err(crate::typechecker::arena_failure)?
            .kind
            .clone();
        let _: () = match kind {
            TypedStmtKind::Block(stmts) => {
                // Blocks change lexical lookup without introducing a closure
                // boundary. Deferred references retain their binding source.
                let locals = self.frames.last().map(|frame| frame.locals.clone());
                for s in stmts {
                    self.walk_stmt(s)?;
                }
                if let (Some(frame), Some(locals)) = (self.frames.last_mut(), locals) {
                    frame.locals = locals;
                }
            }
            TypedStmtKind::Let {
                name, ty, value, ..
            } => {
                self.walk_expr(value)?;
                self.bind(LocalBinding {
                    name_ident: name,
                    ty,
                    mutable: true,
                    source: BindingSource::Let(id),
                })?;
            }
            TypedStmtKind::Const {
                name, ty, value, ..
            } => {
                self.walk_expr(value)?;
                self.bind(LocalBinding {
                    name_ident: name,
                    ty,
                    mutable: false,
                    source: BindingSource::Const,
                })?;
            }
            TypedStmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                self.walk_expr(condition)?;
                self.walk_stmt(then_block)?;
                if let Some(eb) = else_block {
                    self.walk_stmt(eb)?;
                }
            }
            TypedStmtKind::While { condition, body } => {
                self.walk_expr(condition)?;
                self.walk_stmt(body)?;
            }
            TypedStmtKind::For {
                init,
                condition,
                update,
                body,
            } => {
                // Init binding's scope is the for-body.
                self.frames.push(Frame::default());
                if let Some(i) = init {
                    self.walk_stmt(i)?;
                }
                if let Some(c) = condition {
                    self.walk_expr(c)?;
                }
                if let Some(u) = update {
                    self.walk_stmt(u)?;
                }
                self.walk_stmt(body)?;
                self.pop_frame()?;
            }
            TypedStmtKind::ForOf {
                name,
                element_ty,
                iter,
                body,
                binding_kind,
                kind: _,
            } => {
                self.walk_expr(iter)?;
                self.frames.push(Frame::default());
                self.bind(LocalBinding {
                    name_ident: name,
                    ty: element_ty,
                    mutable: matches!(binding_kind, crate::BindingKind::Let),
                    source: BindingSource::Const,
                })?;
                self.walk_stmt(body)?;
                self.pop_frame()?;
            }
            TypedStmtKind::DoWhile { body, condition } => {
                self.walk_stmt(body)?;
                self.walk_expr(condition)?;
            }
            TypedStmtKind::Switch {
                discriminant,
                cases,
                default,
                ..
            } => {
                self.walk_expr(discriminant)?;
                for comparison in cases
                    .iter()
                    .flat_map(crate::TypedSwitchCase::label_comparisons)
                {
                    self.walk_expr(comparison)?;
                }
                for case in cases {
                    self.walk_stmt(case.body)?;
                }
                if let Some(d) = default {
                    self.walk_stmt(d)?;
                }
            }
            TypedStmtKind::Break | TypedStmtKind::Continue => {}
            TypedStmtKind::ReboxLocal { ident, .. } => {
                let binding = self.require_source(&ident.name)?;
                self.resolved.writes.insert(id, binding.name_ident.clone());
            }
            TypedStmtKind::Return(value) => {
                if let Some(v) = value {
                    self.walk_expr(v)?;
                }
            }
            TypedStmtKind::Expr(e) => self.walk_expr(e)?,
            TypedStmtKind::AssignLocal { ident, value, .. } => {
                self.walk_expr(value)?;
                let binding = self.require_source(&ident.name)?;
                self.resolved.writes.insert(id, binding.name_ident.clone());
                self.mark_cross_frame_capture(&ident.name, &binding)?;
                self.pending.push(Pending::AssignLocal(id, binding.source));
            }
            TypedStmtKind::AssignGlobal { value, .. } => self.walk_expr(value)?,
            TypedStmtKind::AssignField {
                receiver, value, ..
            } => {
                self.walk_expr(receiver)?;
                self.walk_expr(value)?;
            }
            TypedStmtKind::AssignIndex {
                receiver,
                index,
                value,
                ..
            } => {
                self.walk_expr(receiver)?;
                self.walk_expr(index)?;
                self.walk_expr(value)?;
            }
            TypedStmtKind::NarrowRegion { source, body, .. } => {
                // Shadow binding is synthetic — no capture implications.
                self.walk_expr(source)?;
                self.walk_stmt(body)?;
            }
            TypedStmtKind::Throw { value } => self.walk_expr(value)?,
            TypedStmtKind::Try {
                body,
                catches,
                finally,
            } => {
                // the try body, each catch body, and finally body
                // are independent lexical scopes. A catch binding
                // `e` lives only inside its own catch body (mirrors
                // ForOf's loop variable). Closures capturing `e` flip
                // its `boxed` flag the same way ForOf handles its
                // loop var.
                self.walk_stmt(body)?;
                for (index, clause) in catches.iter().enumerate() {
                    self.frames.push(Frame::default());
                    self.bind(LocalBinding {
                        name_ident: clause.binding.clone(),
                        ty: clause.ty.clone(),
                        mutable: true,
                        source: BindingSource::Catch {
                            try_stmt: id,
                            index,
                        },
                    })?;
                    self.walk_stmt(clause.body)?;
                    self.pop_frame()?;
                }
                if let Some(f) = finally {
                    self.walk_stmt(f)?;
                }
            }
        };
        Ok(())
    }

    fn walk_expr(&mut self, id: ExprId) -> Result<(), crate::compiler_error::CompilerFailure> {
        let expr = self
            .ta
            .try_expr(id)
            .map_err(crate::typechecker::arena_failure)?;
        let span = expr.span;
        let kind = expr.kind.clone();
        let _: () = match kind {
            TypedExprKind::LocalRef { ident, .. } => {
                let b = self
                    .require_source(&ident.name)
                    .map_err(|failure| failure.with_span(span))?;
                self.resolved.reads.insert(id, b.name_ident.clone());
                self.mark_cross_frame_capture(&ident.name, &b)?;
                self.pending.push(Pending::LocalRef(id, b.source));
            }
            // `this` inside a closure is a capture of the enclosing member's
            // receiver; in the member body itself this resolves same-frame and
            // does nothing.
            TypedExprKind::This => {
                let b = self.require_source(THIS_BINDING)?;
                self.mark_cross_frame_capture(THIS_BINDING, &b)?;
            }
            TypedExprKind::LocalNarrowRef { path, .. } => {
                if path.chain.is_empty()
                    && let super::infer::narrowing::BindingId::Local { name, .. } = &path.root
                {
                    let binding = self
                        .require_source(name)
                        .map_err(|failure| failure.with_span(span))?;
                    self.resolved.reads.insert(id, binding.name_ident);
                }
            }
            TypedExprKind::Closure { params, body, .. } => {
                self.frames.push(Frame::default());
                if self.ta.closure_this.contains_key(&id) {
                    self.bind(LocalBinding {
                        name_ident: Ident {
                            name: THIS_BINDING.to_string(),
                            span: self
                                .ta
                                .try_expr(id)
                                .map_err(crate::typechecker::arena_failure)?
                                .span,
                        },
                        ty: Type::Unknown,
                        mutable: false,
                        source: BindingSource::Const,
                    })?;
                }
                if let Some(name) = self.ta.closure_names.get(&id).cloned() {
                    self.bind(LocalBinding {
                        name_ident: name,
                        ty: self
                            .ta
                            .try_expr(id)
                            .map_err(crate::typechecker::arena_failure)?
                            .ty
                            .clone(),
                        mutable: false,
                        source: BindingSource::Const,
                    })?;
                }
                for (i, p) in params.iter().enumerate() {
                    self.bind(LocalBinding {
                        name_ident: p.name.clone(),
                        ty: p.ty.clone(),
                        mutable: true,
                        source: BindingSource::Param {
                            owner: ParamOwner::Closure(id),
                            index: i,
                        },
                    })?;
                }
                match body {
                    ClosureBody::Expr(e) => self.walk_expr(e)?,
                    ClosureBody::Block(b) => self.walk_stmt(b)?,
                }
                let frame = self.frames.pop().ok_or_else(|| {
                    crate::typechecker::invariant_failure("missing closure capture frame")
                })?;
                let TypedExprKind::Closure { captured, .. } = &mut self
                    .ta
                    .try_expr_mut(id)
                    .map_err(crate::typechecker::arena_failure)?
                    .kind
                else {
                    return Err(crate::typechecker::invariant_failure(
                        "capture closure kind changed",
                    ));
                };
                *captured = frame.captures;
            }
            TypedExprKind::Binary { lhs, rhs, .. } => {
                self.walk_expr(lhs)?;
                self.walk_expr(rhs)?;
            }
            TypedExprKind::EffectThen { effect, result } => {
                self.walk_expr(effect)?;
                self.walk_expr(result)?;
            }
            TypedExprKind::Sequence { stmts, result } => {
                for stmt in stmts {
                    self.walk_stmt(stmt)?;
                }
                self.walk_expr(result)?;
            }
            TypedExprKind::Unary { operand, .. } => self.walk_expr(operand)?,
            TypedExprKind::TypeofTag { value, .. } | TypedExprKind::InstanceOf { value, .. } => {
                self.walk_expr(value)?;
            }
            TypedExprKind::Call { args, .. } | TypedExprKind::McpCall { args, .. } => {
                for a in args {
                    self.walk_expr(a)?;
                }
            }
            // `super.m()` dispatches on the receiver without naming `this`, so
            // it captures it just like an explicit `this` would.
            TypedExprKind::SuperCtorCall { args, .. }
            | TypedExprKind::SuperMethodCall { args, .. } => {
                for a in args {
                    self.walk_expr(a)?;
                }
                {
                    let b = self.require_source(THIS_BINDING)?;
                    self.mark_cross_frame_capture(THIS_BINDING, &b)?;
                }
            }
            TypedExprKind::CallClosure { callee, args } => {
                self.walk_expr(callee)?;
                for a in args {
                    self.walk_expr(a)?;
                }
            }
            TypedExprKind::GenericCall { args, .. } => {
                for a in args {
                    self.walk_expr(a.expr)?;
                }
            }
            TypedExprKind::MethodCall { receiver, args, .. } => {
                self.walk_expr(receiver)?;
                for a in args {
                    self.walk_expr(a)?;
                }
            }
            TypedExprKind::GenericMethodCall { receiver, args, .. } => {
                self.walk_expr(receiver)?;
                for a in args {
                    self.walk_expr(a.expr)?;
                }
            }
            TypedExprKind::IntrinsicCall { args, .. } => {
                for a in args {
                    self.walk_expr(a)?;
                }
            }
            TypedExprKind::ObjectLiteral { members, .. } => {
                for member in members {
                    for expression in member.expressions() {
                        self.walk_expr(expression)?;
                    }
                }
            }
            TypedExprKind::ArrayLiteral { elements, .. } => {
                for e in elements {
                    self.walk_expr(e.expr_id())?;
                }
            }
            TypedExprKind::TupleLiteral { elements, .. } => {
                for e in elements {
                    self.walk_expr(e)?;
                }
            }
            TypedExprKind::FieldAccess { receiver, .. }
            | TypedExprKind::InterfacePropertyAccess { receiver, .. } => {
                self.walk_expr(receiver)?;
            }
            TypedExprKind::IndexAccess { receiver, index } => {
                self.walk_expr(receiver)?;
                self.walk_expr(index)?;
            }
            TypedExprKind::Narrowed { source, inner, .. } => {
                self.walk_expr(source)?;
                self.walk_expr(inner)?;
            }
            TypedExprKind::Ternary { cond, then_, else_ } => {
                self.walk_expr(cond)?;
                self.walk_expr(then_)?;
                self.walk_expr(else_)?;
            }
            TypedExprKind::NullishCoalesce { lhs, rhs } => {
                self.walk_expr(lhs)?;
                self.walk_expr(rhs)?;
            }
            TypedExprKind::OptionalChain { base, parts } => {
                self.walk_expr(base)?;
                for part in parts {
                    match part {
                        TypedChainPart::Index { idx, .. } => self.walk_expr(idx)?,
                        TypedChainPart::Call { args, .. }
                        | TypedChainPart::MethodCall { args, .. } => {
                            for a in args {
                                self.walk_expr(a)?;
                            }
                        }
                        TypedChainPart::Field { .. }
                        | TypedChainPart::InterfaceProperty { .. }
                        | TypedChainPart::NonNull { .. } => {}
                    }
                }
            }
            TypedExprKind::PostfixUnary { target, .. } => match target {
                crate::PostfixTarget::Local { ident, .. } => {
                    let b = self.require_source(&ident.name)?;
                    self.resolved.reads.insert(id, b.name_ident.clone());
                    self.mark_cross_frame_capture(&ident.name, &b)?;
                    self.pending.push(Pending::PostfixLocal(id, b.source));
                }
                crate::PostfixTarget::Global { .. } => {}
                crate::PostfixTarget::Field { receiver, .. } => self.walk_expr(receiver)?,
                crate::PostfixTarget::Index {
                    receiver, index, ..
                } => {
                    self.walk_expr(receiver)?;
                    self.walk_expr(index)?;
                }
            },
            TypedExprKind::NonNullAssert { value } | TypedExprKind::Cast { value, .. } => {
                self.walk_expr(value)?;
            }
            TypedExprKind::Number(_)
            | TypedExprKind::BigInt(_)
            | TypedExprKind::String(_)
            | TypedExprKind::Boolean(_)
            | TypedExprKind::Null
            | TypedExprKind::Regex { .. }
            | TypedExprKind::GlobalRef { .. }
            | TypedExprKind::FunctionRef { .. }
            | TypedExprKind::NumberEnumMember { .. }
            | TypedExprKind::StringEnumMember { .. } => {}
        };
        Ok(())
    }

    fn bind(
        &mut self,
        binding: LocalBinding,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        let frame = self
            .frames
            .last_mut()
            .ok_or_else(|| crate::typechecker::invariant_failure("missing capture scope"))?;
        frame
            .locals
            .insert(binding.name_ident.name.clone(), binding);
        Ok(())
    }

    fn pop_frame(&mut self) -> Result<Frame, crate::compiler_error::CompilerFailure> {
        self.frames
            .pop()
            .ok_or_else(|| crate::typechecker::invariant_failure("capture scope push/pop mismatch"))
    }

    /// Returns binding regardless of depth; callers handle cross-frame via [`mark_cross_frame_capture`].
    fn resolve_source(&self, name: &str) -> Option<LocalBinding> {
        self.frames
            .iter()
            .rev()
            .find_map(|frame| frame.locals.get(name).cloned())
    }

    fn require_source(
        &self,
        name: &str,
    ) -> Result<LocalBinding, crate::compiler_error::CompilerFailure> {
        self.resolve_source(name).ok_or_else(|| {
            crate::typechecker::invariant_failure(format!("missing capture binding `{name}`"))
        })
    }

    /// Forwards capture through intermediate frames and boxes the source binding.
    /// No-op for same-frame — those writes are deferred to pass 2 (`apply_pending`).
    fn mark_cross_frame_capture(
        &mut self,
        name: &str,
        binding: &LocalBinding,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        let defining_idx = self
            .frames
            .iter()
            .rposition(|f| f.locals.contains_key(name))
            .ok_or_else(|| {
                crate::typechecker::invariant_failure("missing resolved capture binding")
            })?;
        let last_idx = self.frames.len() - 1;
        if defining_idx == last_idx {
            return Ok(());
        }
        for f in self.frames.iter_mut().skip(defining_idx + 1) {
            if f.captured_names.insert(name.to_string()) {
                f.captures.push(CapturedVar {
                    name: binding.name_ident.clone(),
                    ty: binding.ty.clone(),
                    boxed: binding.mutable,
                });
            }
        }
        let _: () = if binding.mutable {
            let source = binding.source.clone();
            self.mark_source_boxed(&source)?;
        };
        Ok(())
    }

    fn mark_source_boxed(
        &mut self,
        source: &BindingSource,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        let _: () = match source {
            BindingSource::Let(sid) => {
                let TypedStmtKind::Let { boxed, .. } = &mut self
                    .ta
                    .try_stmt_mut(*sid)
                    .map_err(crate::typechecker::arena_failure)?
                    .kind
                else {
                    return Err(crate::typechecker::invariant_failure(
                        "capture source is not a let binding",
                    ));
                };
                *boxed = true;
            }
            BindingSource::Catch { try_stmt, index } => {
                self.catch_clause_mut(*try_stmt, *index)?.boxed = true;
            }
            BindingSource::Const => {} // const captures are copied; never boxed
            BindingSource::Param { owner, index } => match owner {
                ParamOwner::Function(fidx) => {
                    let param = self
                        .ta
                        .functions
                        .get_mut(*fidx)
                        .and_then(|function| function.params.get_mut(*index))
                        .ok_or_else(|| {
                            crate::typechecker::invariant_failure(
                                "missing function capture parameter",
                            )
                        })?;
                    param.boxed = true;
                }
                ParamOwner::Closure(eid) => {
                    let TypedExprKind::Closure { params, .. } = &mut self
                        .ta
                        .try_expr_mut(*eid)
                        .map_err(crate::typechecker::arena_failure)?
                        .kind
                    else {
                        return Err(crate::typechecker::invariant_failure(
                            "capture parameter owner is not a closure",
                        ));
                    };
                    let param = params.get_mut(*index).ok_or_else(|| {
                        crate::typechecker::invariant_failure("missing closure capture parameter")
                    })?;
                    param.boxed = true;
                }
                ParamOwner::ClassMember(member) => {
                    let param = member.param_mut(self.ta, *index).ok_or_else(|| {
                        crate::typechecker::invariant_failure(
                            "missing class member capture parameter",
                        )
                    })?;
                    param.boxed = true;
                }
            },
        };
        Ok(())
    }

    fn catch_clause(
        &self,
        try_stmt: StmtId,
        index: usize,
    ) -> Result<&crate::TypedCatchClause, crate::compiler_error::CompilerFailure> {
        let TypedStmtKind::Try { catches, .. } = &self
            .ta
            .try_stmt(try_stmt)
            .map_err(crate::typechecker::arena_failure)?
            .kind
        else {
            return Err(crate::typechecker::invariant_failure(
                "capture source is not a try statement",
            ));
        };
        catches
            .get(index)
            .ok_or_else(|| crate::typechecker::invariant_failure("missing catch capture binding"))
    }

    fn catch_clause_mut(
        &mut self,
        try_stmt: StmtId,
        index: usize,
    ) -> Result<&mut crate::TypedCatchClause, crate::compiler_error::CompilerFailure> {
        let TypedStmtKind::Try { catches, .. } = &mut self
            .ta
            .try_stmt_mut(try_stmt)
            .map_err(crate::typechecker::arena_failure)?
            .kind
        else {
            return Err(crate::typechecker::invariant_failure(
                "capture source is not a try statement",
            ));
        };
        catches
            .get_mut(index)
            .ok_or_else(|| crate::typechecker::invariant_failure("missing catch capture binding"))
    }

    /// Deferred so same-frame refs before a capturing closure still see the final `boxed` flag.
    fn apply_pending(&mut self) -> Result<(), crate::compiler_error::CompilerFailure> {
        let pending = std::mem::take(&mut self.pending);
        for p in pending {
            match p {
                Pending::LocalRef(eid, source) => {
                    let boxed = self.source_is_boxed(&source)?;
                    let TypedExprKind::LocalRef { boxed: slot, .. } = &mut self
                        .ta
                        .try_expr_mut(eid)
                        .map_err(crate::typechecker::arena_failure)?
                        .kind
                    else {
                        return Err(crate::typechecker::invariant_failure(
                            "pending capture target kind changed",
                        ));
                    };
                    *slot = boxed;
                }
                Pending::AssignLocal(sid, source) => {
                    let boxed = self.source_is_boxed(&source)?;
                    let TypedStmtKind::AssignLocal { boxed: slot, .. } = &mut self
                        .ta
                        .try_stmt_mut(sid)
                        .map_err(crate::typechecker::arena_failure)?
                        .kind
                    else {
                        return Err(crate::typechecker::invariant_failure(
                            "pending capture target kind changed",
                        ));
                    };
                    *slot = boxed;
                }
                Pending::PostfixLocal(eid, source) => {
                    let boxed = self.source_is_boxed(&source)?;
                    let TypedExprKind::PostfixUnary {
                        target: crate::PostfixTarget::Local { boxed: slot, .. },
                        ..
                    } = &mut self
                        .ta
                        .try_expr_mut(eid)
                        .map_err(crate::typechecker::arena_failure)?
                        .kind
                    else {
                        return Err(crate::typechecker::invariant_failure(
                            "pending capture target kind changed",
                        ));
                    };
                    *slot = boxed;
                }
            }
        }
        Ok(())
    }

    fn source_is_boxed(
        &self,
        source: &BindingSource,
    ) -> Result<bool, crate::compiler_error::CompilerFailure> {
        Ok(match source {
            BindingSource::Let(sid) => match &self
                .ta
                .try_stmt(*sid)
                .map_err(crate::typechecker::arena_failure)?
                .kind
            {
                TypedStmtKind::Let { boxed, .. } => *boxed,
                _ => {
                    return Err(crate::typechecker::invariant_failure(
                        "capture source is not a let binding",
                    ));
                }
            },
            BindingSource::Catch { try_stmt, index } => self.catch_clause(*try_stmt, *index)?.boxed,
            BindingSource::Const => false,
            BindingSource::Param { owner, index } => match owner {
                ParamOwner::Function(fidx) => {
                    self.ta
                        .functions
                        .get(*fidx)
                        .and_then(|f| f.params.get(*index))
                        .ok_or_else(|| {
                            crate::typechecker::invariant_failure(
                                "missing function capture parameter",
                            )
                        })?
                        .boxed
                }
                ParamOwner::Closure(eid) => match &self
                    .ta
                    .try_expr(*eid)
                    .map_err(crate::typechecker::arena_failure)?
                    .kind
                {
                    TypedExprKind::Closure { params, .. } => {
                        params
                            .get(*index)
                            .ok_or_else(|| {
                                crate::typechecker::invariant_failure(
                                    "missing closure capture parameter",
                                )
                            })?
                            .boxed
                    }
                    _ => {
                        return Err(crate::typechecker::invariant_failure(
                            "capture parameter owner is not a closure",
                        ));
                    }
                },
                ParamOwner::ClassMember(member) => {
                    member
                        .param(self.ta, *index)
                        .ok_or_else(|| {
                            crate::typechecker::invariant_failure(
                                "missing class member capture parameter",
                            )
                        })?
                        .boxed
                }
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::capture;
    use crate::{
        Asi, CapturedVar, ClosureBody, ExprId, StmtId, Token, TokenKind, Type, TypedAst,
        TypedChainPart, TypedExprKind, TypedStmtKind, infer, parse,
    };

    #[test]
    fn missing_capture_parameter_is_a_fatal_failure() {
        let mut ta = typecheck("function main(): number { return 1; }");
        let mut state = super::State {
            ta: &mut ta,
            frames: Vec::new(),
            pending: Vec::new(),
            resolved: super::ResolvedLocals::default(),
        };
        let source = super::BindingSource::Param {
            owner: super::ParamOwner::Function(0),
            index: usize::MAX,
        };
        assert!(matches!(
            state.source_is_boxed(&source),
            Err(crate::compiler_error::CompilerFailure::Internal { .. })
        ));
        assert!(matches!(
            state.mark_source_boxed(&source),
            Err(crate::compiler_error::CompilerFailure::Internal { .. })
        ));
    }

    #[test]
    fn missing_local_binding_stops_capture_and_a_fresh_pass_succeeds() {
        let source = "function main(value: number): number { return value; }";
        let mut ta = typecheck(source);
        let mut changed = false;
        for id in ta.expr_ids().unwrap() {
            if let TypedExprKind::LocalRef { ident, .. } = &mut ta.try_expr_mut(id).unwrap().kind {
                ident.name = "missing".into();
                changed = true;
                break;
            }
        }
        assert!(changed);
        assert!(matches!(
            capture(ta),
            Err(crate::compiler_error::CompilerFailure::Internal { .. })
        ));
        assert!(capture(typecheck(source)).is_ok());
    }

    #[test]
    fn body_exit_narrowings_do_not_leak_into_later_declarations() {
        for declaration in [
            "function first(x: { ok: boolean }): void { if (!x.ok) throw new Error(\"bad\"); }",
            "class C { first(x: { ok: boolean }): void { if (!x.ok) throw new Error(\"bad\"); } second(): number { return 1; } }",
            "class C { static first(x: { ok: boolean }): void { if (!x.ok) throw new Error(\"bad\"); } static second(): number { return 1; } }",
            "class C { constructor(x: { ok: boolean }) { if (!x.ok) throw new Error(\"bad\"); } second(): number { return 1; } }",
            "class C { set value(x: { ok: boolean }) { if (!x.ok) throw new Error(\"bad\"); } second(): number { return 1; } }",
        ] {
            let source = format!("{declaration} function main(): number {{ return 1; }}");
            assert!(capture(typecheck(&source)).is_ok(), "{source}");
        }
    }

    fn typecheck(source: &str) -> TypedAst {
        let mut asi = Asi::new(source, crate::FileId(0));
        let mut tokens: Vec<Token> = Vec::new();
        loop {
            let tok = asi.next_token();
            let is_eof = matches!(tok.kind, TokenKind::Eof);
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        let lex_diags = asi.into_diagnostics();
        assert!(
            lex_diags.is_empty(),
            "unexpected lexer diags: {lex_diags:?}"
        );
        let (ast, parse_diags) = parse(source, tokens, crate::FileId(0));
        assert!(
            parse_diags.is_empty(),
            "unexpected parser diags: {parse_diags:?}"
        );
        let (prelude_defs, host_defs, _) =
            crate::runtime::prelude::cached_runtime_package_declarations();
        let mut packages = Vec::with_capacity(prelude_defs.len() + host_defs.len());
        packages.extend(prelude_defs.iter());
        packages.extend(host_defs.iter());
        let (ta, infer_diags) = infer(source, "main", &ast, &packages);
        assert!(
            infer_diags.is_empty(),
            "unexpected infer diags: {infer_diags:?}"
        );
        ta
    }

    fn run(source: &str) -> TypedAst {
        let mut ta = typecheck(source);
        ta = capture(ta).unwrap();
        ta
    }

    #[derive(Debug, PartialEq)]
    struct Flag {
        kind: &'static str,
        name: String,
        boxed: bool,
    }

    fn collect_flags(ta: &TypedAst) -> Vec<Flag> {
        let mut out = Vec::new();
        for f in &ta.functions {
            for p in &f.params {
                out.push(Flag {
                    kind: "Param",
                    name: p.name.name.clone(),
                    boxed: p.boxed,
                });
            }
            walk_stmt(ta, f.body, &mut out);
        }
        for &id in &ta.top_level_statements {
            walk_stmt(ta, id, &mut out);
        }
        out
    }

    fn walk_stmt(ta: &TypedAst, id: StmtId, out: &mut Vec<Flag>) {
        match &ta.try_stmt(id).unwrap().kind {
            TypedStmtKind::Let {
                name, value, boxed, ..
            } => {
                out.push(Flag {
                    kind: "Let",
                    name: name.name.clone(),
                    boxed: *boxed,
                });
                walk_expr(ta, *value, out);
            }
            TypedStmtKind::Const { value, .. } => walk_expr(ta, *value, out),
            TypedStmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                walk_expr(ta, *condition, out);
                walk_stmt(ta, *then_block, out);
                if let Some(eb) = else_block {
                    walk_stmt(ta, *eb, out);
                }
            }
            TypedStmtKind::While { condition, body } => {
                walk_expr(ta, *condition, out);
                walk_stmt(ta, *body, out);
            }
            TypedStmtKind::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    walk_stmt(ta, *i, out);
                }
                if let Some(c) = condition {
                    walk_expr(ta, *c, out);
                }
                if let Some(u) = update {
                    walk_stmt(ta, *u, out);
                }
                walk_stmt(ta, *body, out);
            }
            TypedStmtKind::ForOf { iter, body, .. } => {
                walk_expr(ta, *iter, out);
                walk_stmt(ta, *body, out);
            }
            TypedStmtKind::DoWhile { body, condition } => {
                walk_stmt(ta, *body, out);
                walk_expr(ta, *condition, out);
            }
            TypedStmtKind::Switch {
                discriminant,
                cases,
                default,
                ..
            } => {
                walk_expr(ta, *discriminant, out);
                for comparison in cases
                    .iter()
                    .flat_map(crate::TypedSwitchCase::label_comparisons)
                {
                    walk_expr(ta, comparison, out);
                }
                for case in cases {
                    walk_stmt(ta, case.body, out);
                }
                if let Some(d) = default {
                    walk_stmt(ta, *d, out);
                }
            }
            TypedStmtKind::Break | TypedStmtKind::Continue | TypedStmtKind::ReboxLocal { .. } => {}
            TypedStmtKind::Return(value) => {
                if let Some(v) = value {
                    walk_expr(ta, *v, out);
                }
            }
            TypedStmtKind::Expr(e) => walk_expr(ta, *e, out),
            TypedStmtKind::Block(stmts) => {
                for &s in stmts {
                    walk_stmt(ta, s, out);
                }
            }
            TypedStmtKind::AssignLocal {
                ident,
                value,
                boxed,
                ..
            } => {
                out.push(Flag {
                    kind: "AssignLocal",
                    name: ident.name.clone(),
                    boxed: *boxed,
                });
                walk_expr(ta, *value, out);
            }
            TypedStmtKind::AssignGlobal { value, .. } => walk_expr(ta, *value, out),
            TypedStmtKind::AssignField {
                receiver, value, ..
            } => {
                walk_expr(ta, *receiver, out);
                walk_expr(ta, *value, out);
            }
            TypedStmtKind::AssignIndex {
                receiver,
                index,
                value,
                ..
            } => {
                walk_expr(ta, *receiver, out);
                walk_expr(ta, *index, out);
                walk_expr(ta, *value, out);
            }
            TypedStmtKind::NarrowRegion { source, body, .. } => {
                walk_expr(ta, *source, out);
                walk_stmt(ta, *body, out);
            }
            TypedStmtKind::Throw { value } => walk_expr(ta, *value, out),
            TypedStmtKind::Try {
                body,
                catches,
                finally,
            } => {
                walk_stmt(ta, *body, out);
                for c in catches {
                    out.push(Flag {
                        kind: "Catch",
                        name: c.binding.name.clone(),
                        boxed: c.boxed,
                    });
                    walk_stmt(ta, c.body, out);
                }
                if let Some(f) = finally {
                    walk_stmt(ta, *f, out);
                }
            }
        }
    }

    fn walk_expr(ta: &TypedAst, id: ExprId, out: &mut Vec<Flag>) {
        match &ta.try_expr(id).unwrap().kind {
            TypedExprKind::LocalRef { ident, boxed } => out.push(Flag {
                kind: "LocalRef",
                name: ident.name.clone(),
                boxed: *boxed,
            }),
            TypedExprKind::LocalNarrowRef { binding, .. } => out.push(Flag {
                kind: "LocalNarrowRef",
                name: binding.name.clone(),
                boxed: false,
            }),
            TypedExprKind::GlobalRef { .. } | TypedExprKind::FunctionRef { .. } => {}
            TypedExprKind::Number(_)
            | TypedExprKind::BigInt(_)
            | TypedExprKind::String(_)
            | TypedExprKind::Boolean(_)
            | TypedExprKind::Null
            | TypedExprKind::This
            | TypedExprKind::Regex { .. }
            | TypedExprKind::NumberEnumMember { .. }
            | TypedExprKind::StringEnumMember { .. } => {}
            TypedExprKind::Binary { lhs, rhs, .. } => {
                walk_expr(ta, *lhs, out);
                walk_expr(ta, *rhs, out);
            }
            TypedExprKind::EffectThen { effect, result } => {
                walk_expr(ta, *effect, out);
                walk_expr(ta, *result, out);
            }
            TypedExprKind::Sequence { stmts, result } => {
                for &stmt in stmts {
                    walk_stmt(ta, stmt, out);
                }
                walk_expr(ta, *result, out);
            }
            TypedExprKind::Unary { operand, .. } => walk_expr(ta, *operand, out),
            TypedExprKind::TypeofTag { value, .. } | TypedExprKind::InstanceOf { value, .. } => {
                walk_expr(ta, *value, out);
            }
            TypedExprKind::Call { args, .. }
            | TypedExprKind::McpCall { args, .. }
            | TypedExprKind::SuperCtorCall { args, .. }
            | TypedExprKind::SuperMethodCall { args, .. } => {
                for &a in args {
                    walk_expr(ta, a, out);
                }
            }
            TypedExprKind::CallClosure { callee, args } => {
                walk_expr(ta, *callee, out);
                for &a in args {
                    walk_expr(ta, a, out);
                }
            }
            TypedExprKind::GenericCall { args, .. } => {
                for a in args {
                    walk_expr(ta, a.expr, out);
                }
            }
            TypedExprKind::MethodCall { receiver, args, .. } => {
                walk_expr(ta, *receiver, out);
                for &a in args {
                    walk_expr(ta, a, out);
                }
            }
            TypedExprKind::GenericMethodCall { receiver, args, .. } => {
                walk_expr(ta, *receiver, out);
                for a in args {
                    walk_expr(ta, a.expr, out);
                }
            }
            TypedExprKind::IntrinsicCall { args, .. } => {
                for &a in args {
                    walk_expr(ta, a, out);
                }
            }
            TypedExprKind::ObjectLiteral { members, .. } => {
                for member in members {
                    for expression in member.expressions() {
                        walk_expr(ta, expression, out);
                    }
                }
            }
            TypedExprKind::ArrayLiteral { elements, .. } => {
                for e in elements {
                    walk_expr(ta, e.expr_id(), out);
                }
            }
            TypedExprKind::TupleLiteral { elements, .. } => {
                for &e in elements {
                    walk_expr(ta, e, out);
                }
            }
            TypedExprKind::FieldAccess { receiver, .. }
            | TypedExprKind::InterfacePropertyAccess { receiver, .. } => {
                walk_expr(ta, *receiver, out);
            }
            TypedExprKind::IndexAccess { receiver, index } => {
                walk_expr(ta, *receiver, out);
                walk_expr(ta, *index, out);
            }
            TypedExprKind::Closure { params, body, .. } => {
                for p in params {
                    out.push(Flag {
                        kind: "Param",
                        name: p.name.name.clone(),
                        boxed: p.boxed,
                    });
                }
                match *body {
                    ClosureBody::Expr(e) => walk_expr(ta, e, out),
                    ClosureBody::Block(b) => walk_stmt(ta, b, out),
                }
            }
            TypedExprKind::Narrowed { source, inner, .. } => {
                walk_expr(ta, *source, out);
                walk_expr(ta, *inner, out);
            }
            TypedExprKind::Ternary { cond, then_, else_ } => {
                walk_expr(ta, *cond, out);
                walk_expr(ta, *then_, out);
                walk_expr(ta, *else_, out);
            }
            TypedExprKind::NullishCoalesce { lhs, rhs } => {
                walk_expr(ta, *lhs, out);
                walk_expr(ta, *rhs, out);
            }
            TypedExprKind::OptionalChain { base, parts } => {
                walk_expr(ta, *base, out);
                for part in parts {
                    match part {
                        TypedChainPart::Index { idx, .. } => walk_expr(ta, *idx, out),
                        TypedChainPart::Call { args, .. }
                        | TypedChainPart::MethodCall { args, .. } => {
                            for a in args {
                                walk_expr(ta, *a, out);
                            }
                        }
                        TypedChainPart::Field { .. }
                        | TypedChainPart::InterfaceProperty { .. }
                        | TypedChainPart::NonNull { .. } => {}
                    }
                }
            }
            TypedExprKind::PostfixUnary { target, .. } => match target {
                crate::PostfixTarget::Local { ident, boxed, .. } => out.push(Flag {
                    kind: "PostfixLocal",
                    name: ident.name.clone(),
                    boxed: *boxed,
                }),
                crate::PostfixTarget::Global { .. } => {}
                crate::PostfixTarget::Field { receiver, .. } => walk_expr(ta, *receiver, out),
                crate::PostfixTarget::Index {
                    receiver, index, ..
                } => {
                    walk_expr(ta, *receiver, out);
                    walk_expr(ta, *index, out);
                }
            },
            TypedExprKind::NonNullAssert { value } | TypedExprKind::Cast { value, .. } => {
                walk_expr(ta, *value, out);
            }
        }
    }

    fn first_closure(ta: &TypedAst) -> (Vec<CapturedVar>, ExprId) {
        for f in &ta.functions {
            if let Some(found) = scan_stmt(ta, f.body) {
                return found;
            }
        }
        for &id in &ta.top_level_statements {
            if let Some(found) = scan_stmt(ta, id) {
                return found;
            }
        }
        panic!("no Closure found in subtree");
    }

    fn scan_stmt(ta: &TypedAst, id: StmtId) -> Option<(Vec<CapturedVar>, ExprId)> {
        match &ta.try_stmt(id).unwrap().kind {
            TypedStmtKind::Let { value, .. }
            | TypedStmtKind::Const { value, .. }
            | TypedStmtKind::AssignLocal { value, .. }
            | TypedStmtKind::AssignGlobal { value, .. } => scan_expr(ta, *value),
            TypedStmtKind::If {
                condition,
                then_block,
                else_block,
            } => scan_expr(ta, *condition)
                .or_else(|| scan_stmt(ta, *then_block))
                .or_else(|| else_block.and_then(|eb| scan_stmt(ta, eb))),
            TypedStmtKind::While { condition, body } => {
                scan_expr(ta, *condition).or_else(|| scan_stmt(ta, *body))
            }
            TypedStmtKind::For {
                init,
                condition,
                update,
                body,
            } => init
                .and_then(|i| scan_stmt(ta, i))
                .or_else(|| condition.and_then(|c| scan_expr(ta, c)))
                .or_else(|| update.and_then(|u| scan_stmt(ta, u)))
                .or_else(|| scan_stmt(ta, *body)),
            TypedStmtKind::ForOf { iter, body, .. } => {
                scan_expr(ta, *iter).or_else(|| scan_stmt(ta, *body))
            }
            TypedStmtKind::DoWhile { body, condition } => {
                scan_stmt(ta, *body).or_else(|| scan_expr(ta, *condition))
            }
            TypedStmtKind::Switch {
                discriminant,
                cases,
                default,
                ..
            } => scan_expr(ta, *discriminant)
                .or_else(|| cases.iter().find_map(|c| scan_stmt(ta, c.body)))
                .or_else(|| default.and_then(|d| scan_stmt(ta, d))),
            TypedStmtKind::Break | TypedStmtKind::Continue | TypedStmtKind::ReboxLocal { .. } => {
                None
            }
            TypedStmtKind::Return(value) => value.and_then(|v| scan_expr(ta, v)),
            TypedStmtKind::Expr(e) => scan_expr(ta, *e),
            TypedStmtKind::Block(stmts) => stmts.iter().find_map(|&s| scan_stmt(ta, s)),
            TypedStmtKind::AssignField {
                receiver, value, ..
            } => scan_expr(ta, *receiver).or_else(|| scan_expr(ta, *value)),
            TypedStmtKind::AssignIndex {
                receiver,
                index,
                value,
                ..
            } => scan_expr(ta, *receiver)
                .or_else(|| scan_expr(ta, *index))
                .or_else(|| scan_expr(ta, *value)),
            TypedStmtKind::NarrowRegion { source, body, .. } => {
                scan_expr(ta, *source).or_else(|| scan_stmt(ta, *body))
            }
            TypedStmtKind::Throw { value } => scan_expr(ta, *value),
            TypedStmtKind::Try {
                body,
                catches,
                finally,
            } => scan_stmt(ta, *body)
                .or_else(|| catches.iter().find_map(|c| scan_stmt(ta, c.body)))
                .or_else(|| finally.and_then(|f| scan_stmt(ta, f))),
        }
    }

    fn scan_expr(ta: &TypedAst, id: ExprId) -> Option<(Vec<CapturedVar>, ExprId)> {
        if let TypedExprKind::Closure { captured, .. } = &ta.try_expr(id).unwrap().kind {
            return Some((captured.clone(), id));
        }
        match &ta.try_expr(id).unwrap().kind {
            TypedExprKind::Binary { lhs, rhs, .. } => {
                scan_expr(ta, *lhs).or_else(|| scan_expr(ta, *rhs))
            }
            TypedExprKind::Unary { operand, .. } => scan_expr(ta, *operand),
            TypedExprKind::TypeofTag { value, .. } | TypedExprKind::InstanceOf { value, .. } => {
                scan_expr(ta, *value)
            }
            TypedExprKind::Call { args, .. } | TypedExprKind::McpCall { args, .. } => {
                args.iter().find_map(|&a| scan_expr(ta, a))
            }
            TypedExprKind::CallClosure { callee, args } => {
                scan_expr(ta, *callee).or_else(|| args.iter().find_map(|&a| scan_expr(ta, a)))
            }
            TypedExprKind::GenericCall { args, .. } => {
                args.iter().find_map(|a| scan_expr(ta, a.expr))
            }
            TypedExprKind::MethodCall { receiver, args, .. } => {
                scan_expr(ta, *receiver).or_else(|| args.iter().find_map(|&a| scan_expr(ta, a)))
            }
            TypedExprKind::GenericMethodCall { receiver, args, .. } => {
                scan_expr(ta, *receiver).or_else(|| args.iter().find_map(|a| scan_expr(ta, a.expr)))
            }
            TypedExprKind::IntrinsicCall { args, .. } => {
                args.iter().find_map(|&a| scan_expr(ta, a))
            }
            TypedExprKind::ObjectLiteral { members, .. } => members
                .iter()
                .flat_map(|member| member.expressions())
                .find_map(|expression| scan_expr(ta, expression)),
            TypedExprKind::ArrayLiteral { elements, .. } => {
                elements.iter().find_map(|e| scan_expr(ta, e.expr_id()))
            }
            TypedExprKind::FieldAccess { receiver, .. }
            | TypedExprKind::InterfacePropertyAccess { receiver, .. } => scan_expr(ta, *receiver),
            TypedExprKind::IndexAccess { receiver, index } => {
                scan_expr(ta, *receiver).or_else(|| scan_expr(ta, *index))
            }
            _ => None,
        }
    }

    #[test]
    fn mvp_program_has_no_boxed_flags() {
        let ta = run(
            "function f(n: number): number { let x: number = n + 1; return x; } let r: number = f(2);",
        );
        let flags = collect_flags(&ta);
        assert!(flags.iter().all(|f| !f.boxed), "{flags:?}");
    }

    #[test]
    fn single_let_captured_is_boxed() {
        let ta = run("function host(): void { let x: number = 0; const g = () => x; }");
        let flags = collect_flags(&ta);
        let x_let = flags
            .iter()
            .find(|f| f.kind == "Let" && f.name == "x")
            .expect("Let x");
        assert!(x_let.boxed, "expected outer let x to be boxed");
        let x_ref = flags
            .iter()
            .find(|f| f.kind == "LocalRef" && f.name == "x")
            .expect("LocalRef x");
        assert!(
            x_ref.boxed,
            "expected LocalRef(x) inside closure to be boxed"
        );

        let (captured, _) = first_closure(&ta);
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].name.name, "x");
        assert_eq!(captured[0].ty, Type::Number);
        assert!(captured[0].boxed);
    }

    #[test]
    fn single_const_captured_is_unboxed() {
        let ta = run("function host(): void { const x: number = 0; const g = () => x; }");
        let flags = collect_flags(&ta);
        let x_ref = flags
            .iter()
            .find(|f| f.kind == "LocalRef" && f.name == "x")
            .expect("LocalRef x");
        assert!(
            !x_ref.boxed,
            "expected LocalRef(x) referencing const to be unboxed"
        );

        let (captured, _) = first_closure(&ta);
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].name.name, "x");
        assert!(!captured[0].boxed, "const captures are copied, not boxed");
    }

    #[test]
    fn param_captured_is_boxed() {
        let ta = run("function host(x: number): void { const g = () => x; }");
        let flags = collect_flags(&ta);
        let x_param = flags
            .iter()
            .find(|f| f.kind == "Param" && f.name == "x")
            .expect("Param x");
        assert!(x_param.boxed, "captured param must be boxed");
    }

    #[test]
    fn write_to_captured_let_is_boxed() {
        let ta =
            run("function host(): void { let x: number = 0; const g = () => { x = x + 1; }; }");
        let flags = collect_flags(&ta);
        let x_let = flags
            .iter()
            .find(|f| f.kind == "Let" && f.name == "x")
            .unwrap();
        assert!(x_let.boxed, "let x must be boxed");
        let assign = flags
            .iter()
            .find(|f| f.kind == "AssignLocal" && f.name == "x")
            .expect("AssignLocal x");
        assert!(assign.boxed, "AssignLocal(x) must be boxed");
        let refs: Vec<&Flag> = flags
            .iter()
            .filter(|f| f.kind == "LocalRef" && f.name == "x")
            .collect();
        assert!(!refs.is_empty(), "expected at least one LocalRef x");
        for r in &refs {
            assert!(r.boxed, "LocalRef(x) inside closure must be boxed");
        }
    }

    #[test]
    fn same_frame_reassign_of_captured_let_is_boxed() {
        let ta = run("function host(): void { let x: number = 0; const g = () => x; x = x + 1; }");
        let flags = collect_flags(&ta);
        let x_let = flags
            .iter()
            .find(|f| f.kind == "Let" && f.name == "x")
            .expect("Let x");
        assert!(
            x_let.boxed,
            "let x must be boxed because closure captures it"
        );
        let assign = flags
            .iter()
            .find(|f| f.kind == "AssignLocal" && f.name == "x")
            .expect("AssignLocal x");
        assert!(
            assign.boxed,
            "same-frame AssignLocal(x) must be boxed (SUB-148)",
        );
        // Every LocalRef(x) — inside the closure body AND inside the
        // outer-frame `x + 1` value — must be boxed.
        let refs: Vec<&Flag> = flags
            .iter()
            .filter(|f| f.kind == "LocalRef" && f.name == "x")
            .collect();
        assert!(
            refs.len() >= 2,
            "expected refs in closure + same-frame value, got {refs:?}"
        );
        for r in &refs {
            assert!(r.boxed, "every LocalRef(x) must be boxed: {r:?}");
        }
    }

    #[test]
    fn same_frame_reassign_appearing_before_closure_is_boxed() {
        let ta = run("function host(): void { let x: number = 0; x = x + 1; const g = () => x; }");
        let flags = collect_flags(&ta);
        let x_let = flags
            .iter()
            .find(|f| f.kind == "Let" && f.name == "x")
            .expect("Let x");
        assert!(x_let.boxed);
        let assign = flags
            .iter()
            .find(|f| f.kind == "AssignLocal" && f.name == "x")
            .expect("AssignLocal x");
        assert!(
            assign.boxed,
            "AssignLocal before inner closure must still be boxed (SUB-148 ordering)",
        );
        for r in flags
            .iter()
            .filter(|f| f.kind == "LocalRef" && f.name == "x")
        {
            assert!(
                r.boxed,
                "LocalRef(x) must be boxed regardless of walk order: {r:?}"
            );
        }
    }

    #[test]
    fn same_frame_param_reassign_with_inner_capture_is_boxed() {
        let ta = run(
            "function f(p: number): void { p = p + 1; const inner = () => p; } \
             function main(): void {}",
        );
        let flags = collect_flags(&ta);
        let p_param = flags
            .iter()
            .find(|f| f.kind == "Param" && f.name == "p")
            .expect("Param p");
        assert!(p_param.boxed, "captured param must be boxed");
        let assign = flags
            .iter()
            .find(|f| f.kind == "AssignLocal" && f.name == "p")
            .expect("AssignLocal p");
        assert!(
            assign.boxed,
            "same-frame AssignLocal(p) for a captured param must be boxed (SUB-148)",
        );
        for r in flags
            .iter()
            .filter(|f| f.kind == "LocalRef" && f.name == "p")
        {
            assert!(r.boxed, "every LocalRef(p) must be boxed: {r:?}");
        }
    }

    #[test]
    fn transitive_capture_forwards_through_intermediate_closure() {
        let ta = run("function host(): void { let x: number = 0; \
              const mid = () => { const inner = () => x; }; }");

        let flags = collect_flags(&ta);
        let x_let = flags
            .iter()
            .find(|f| f.kind == "Let" && f.name == "x")
            .unwrap();
        assert!(x_let.boxed);

        let (mid_captured, mid_id) = first_closure(&ta);
        assert_eq!(mid_captured.len(), 1, "mid forwards `x`");
        assert_eq!(mid_captured[0].name.name, "x");
        assert!(mid_captured[0].boxed);

        let (inner_captured, _) = match &ta.try_expr(mid_id).unwrap().kind {
            TypedExprKind::Closure {
                body: ClosureBody::Block(b),
                ..
            } => scan_stmt(&ta, *b).expect("inner closure"),
            _ => panic!("mid is not a block-body closure"),
        };
        assert_eq!(inner_captured.len(), 1, "inner captures `x`");
        assert_eq!(inner_captured[0].name.name, "x");
        assert!(inner_captured[0].boxed);
    }

    #[test]
    fn shadowed_binding_does_not_capture_outer() {
        let ta = run("function host(): void { let x: number = 0; \
              const g = () => { let x: number = 1; x = x + 1; }; }");
        let flags = collect_flags(&ta);
        let x_lets: Vec<&Flag> = flags
            .iter()
            .filter(|f| f.kind == "Let" && f.name == "x")
            .collect();
        assert_eq!(x_lets.len(), 2, "two `let x` declarations");
        for l in x_lets {
            assert!(!l.boxed, "neither `let x` should be boxed");
        }
        let (captured, _) = first_closure(&ta);
        assert!(captured.is_empty(), "shadowed name doesn't capture");
    }

    #[test]
    fn multiple_captures_recorded_in_order() {
        let ta = run("function host(p: number): void { \
               let a: number = 1; \
               const b: number = 2; \
               const g = () => a + b + p; \
             }");
        let (captured, _) = first_closure(&ta);
        assert_eq!(captured.len(), 3, "{captured:?}");

        let names: Vec<&str> = captured.iter().map(|c| c.name.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "p"]);

        let a = &captured[0];
        let b = &captured[1];
        let p = &captured[2];
        assert!(a.boxed, "let captured → boxed");
        assert!(!b.boxed, "const captured → unboxed");
        assert!(p.boxed, "param captured → boxed");
    }

    #[test]
    fn capture_is_idempotent() {
        let mut ta = typecheck("function host(): void { let x: number = 0; const g = () => x; }");
        ta = capture(ta).unwrap();
        let after_one = format!("{ta:?}");
        ta = capture(ta).unwrap();
        let after_two = format!("{ta:?}");
        assert_eq!(
            after_one, after_two,
            "second run should not change anything"
        );
    }

    #[test]
    fn capture_runs_on_empty_program() {
        let mut ta = typecheck("");
        ta = capture(ta).unwrap();
        assert!(ta.globals.is_empty());
        assert!(ta.functions.is_empty());
        assert!(ta.top_level_statements.is_empty());
    }
}
