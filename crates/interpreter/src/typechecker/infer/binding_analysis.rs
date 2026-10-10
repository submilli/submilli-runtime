//! Lexical binding checks and closure-write analysis before inference.
//! Declaration spans remain stable when inference revisits a loop or generic body.

use crate::compiler_error::CompilerFailure;

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::{Ast, Diagnostic, ExprId, Ident, Severity, Span, StmtId};

#[derive(Default)]
pub(super) struct Analysis {
    pub(super) mutators: HashSet<(String, Span)>,
    /// Names a function body writes that no enclosing block declares: the
    /// module-level bindings functions write.
    pub(super) function_written_globals: HashSet<String>,
    /// Locals, by declaration, that arithmetic is written back to: `x += 1`,
    /// `x++`, `x = x * 2`.
    pub(super) arithmetic_targets: HashSet<(String, Span)>,
    /// Module-level names arithmetic is written back to anywhere.
    pub(super) arithmetic_written_globals: HashSet<String>,
    pub(super) last_assignments: HashMap<Span, u32>,
    /// Declarations, by name span, of the bindings code later reassigns or
    /// adds elements to (`x = …`, `x.push(…)`, `x[i] = …`). An unannotated
    /// `[]` bound to one is an array tsc types from those writes.
    pub(super) grown_bindings: HashSet<Span>,
    /// Nested function declarations, by name span, whose bodies read or write a
    /// `let`/`const` of the block they are declared in, with the last declared
    /// of those. Their closure can exist only once it is declared; any other is
    /// hoisted to the block's start.
    pub(super) nested_function_creation_points: HashMap<Span, Ident>,
    pub(super) diagnostics: Vec<Diagnostic>,
    scopes: Vec<BTreeMap<String, Binding>>,
    function_depth: usize,
    /// Function frames entered by direct invocation, rather than deferred closure creation.
    immediate_function_depths: Vec<usize>,
    assignment_regions: Vec<Span>,
    /// The nested function declarations whose bodies are being scanned: each
    /// one's name span and the index in `scopes` of the block declaring it.
    nested_functions: Vec<(Span, usize)>,
}

#[derive(Clone, Copy)]
struct Binding {
    span: Span,
    function_depth: usize,
    initialized: bool,
    /// A binding initialized in source order, including parameter initializers.
    block_local: bool,
    /// A default can create a closure over a later parameter's shared slot.
    parameter: bool,
}

pub(super) fn analyze(ast: &Ast) -> Result<Analysis, crate::compiler_error::CompileError> {
    let mut analysis = Analysis::default();
    for &id in &ast.top_level {
        visit_stmt(ast, id, &mut analysis).map_err(|fatal| {
            crate::compiler_error::CompileError {
                diagnostics: analysis.diagnostics.clone(),
                fatal: Some(fatal),
            }
        })?;
    }
    Ok(analysis)
}

/// No `_` arm: a statement kind that stops the walk hides every arrow below it,
/// and the resulting narrowing is unsound rather than merely imprecise — the
/// enclosing frame reads a stale shadow while the closure has already written
/// the slot. A new statement kind should fail the build here, not go unscanned.
fn visit_stmt(ast: &Ast, id: StmtId, out: &mut Analysis) -> Result<(), CompilerFailure> {
    use crate::StmtKind;
    let extends_assignment = matches!(
        ast.try_stmt(id).map_err(super::arena_failure)?.kind,
        StmtKind::Let { .. }
            | StmtKind::Assign { .. }
            | StmtKind::CompoundAssign { .. }
            | StmtKind::Const { .. }
            | StmtKind::Expr(_)
            | StmtKind::If { .. }
            | StmtKind::While { .. }
            | StmtKind::DoWhile { .. }
            | StmtKind::For { .. }
            | StmtKind::ForOf { .. }
            | StmtKind::Switch { .. }
            | StmtKind::Try { .. }
            | StmtKind::ClassDecl { .. }
    );
    if extends_assignment {
        out.assignment_regions
            .push(ast.try_stmt(id).map_err(super::arena_failure)?.span);
    }
    match &ast.try_stmt(id).map_err(super::arena_failure)?.kind {
        StmtKind::Assign { target, value } => {
            out.read(target);
            out.write(target);
            out.grow_binding(target);
            if writes_back_arithmetic(ast, None, *value)? {
                out.write_arithmetic(target);
            }
            visit_expr(ast, *value, out)?;
        }
        StmtKind::CompoundAssign {
            target, op, value, ..
        } => {
            out.read(target);
            out.write(target);
            out.grow_binding(target);
            if writes_back_arithmetic(ast, Some(*op), *value)? {
                out.write_arithmetic(target);
            }
            visit_expr(ast, *value, out)?;
        }
        StmtKind::Let { name, value, .. } | StmtKind::Const { name, value, .. } => {
            visit_expr(ast, *value, out)?;
            out.initialize(name);
            out.write(name);
        }
        StmtKind::ObjectRest { name, source, .. } => {
            visit_expr(ast, *source, out)?;
            out.initialize(name);
            out.write(name);
        }
        StmtKind::Function {
            name, params, body, ..
        } => {
            // At the top level there is no scope, and the function is no closure.
            let declaring_scope = out.scopes.len().checked_sub(1);
            if let Some(scope) = declaring_scope {
                out.nested_functions.push((name.span, scope));
            }
            scan_function(ast, params, crate::ArrowBody::Block(*body), out)?;
            if declaring_scope.is_some() {
                out.nested_functions.pop();
            }
        }
        StmtKind::If {
            condition,
            then_block,
            else_block,
        } => {
            visit_expr(ast, *condition, out)?;
            visit_stmt(ast, *then_block, out)?;
            if let Some(e) = else_block {
                visit_stmt(ast, *e, out)?;
            }
        }
        StmtKind::While { condition, body } | StmtKind::DoWhile { body, condition } => {
            visit_expr(ast, *condition, out)?;
            visit_stmt(ast, *body, out)?;
        }
        StmtKind::For {
            init,
            condition,
            update,
            body,
        } => {
            out.scopes.push(Default::default());
            if let Some(init) = init {
                out.reserve_statements(ast, &[*init])?;
            }
            for s in [init, update].into_iter().flatten() {
                visit_stmt(ast, *s, out)?;
            }
            if let Some(c) = condition {
                visit_expr(ast, *c, out)?;
            }
            visit_stmt(ast, *body, out)?;
            out.scopes.pop();
        }
        StmtKind::ForOf {
            name, iter, body, ..
        } => {
            out.scopes.push(Default::default());
            out.declare(name, false);
            visit_iterable(ast, id, *iter, out)?;
            out.initialize(name);
            visit_stmt(ast, *body, out)?;
            out.scopes.pop();
        }
        StmtKind::Switch {
            discriminant,
            cases,
            default,
        } => {
            visit_expr(ast, *discriminant, out)?;
            // The clauses share one scope, as in JavaScript: a name declared in
            // two clauses is a redeclaration, a function is hoisted to the body's
            // start, and a `let`/`const` is uninitialized until its statement.
            let clauses =
                super::switch_stmt::clauses_in_source_order(ast, cases, default.as_ref())?;
            out.scopes.push(Default::default());
            let all_stmts: Vec<StmtId> = clauses
                .iter()
                .flat_map(|c| c.stmts.iter().copied())
                .collect();
            out.reserve_statements(ast, &all_stmts)?;
            for clause in &clauses {
                for &value in clause.values {
                    visit_expr(ast, value, out)?;
                }
                for &stmt in &clause.stmts {
                    visit_stmt(ast, stmt, out)?;
                }
            }
            out.scopes.pop();
        }
        StmtKind::Try {
            body,
            catches,
            finally,
        } => {
            visit_stmt(ast, *body, out)?;
            for clause in catches {
                out.scopes.push(Default::default());
                out.declare(&clause.binding, true);
                scan_body(ast, clause.body, out)?;
                out.scopes.pop();
            }
            if let Some(f) = finally {
                visit_stmt(ast, *f, out)?;
            }
        }
        StmtKind::Return(value) => {
            if let Some(v) = value {
                visit_expr(ast, *v, out)?;
            }
        }
        StmtKind::Throw { value } | StmtKind::Expr(value) => {
            visit_expr(ast, *value, out)?;
        }
        StmtKind::Block(_) => {
            out.scopes.push(Default::default());
            scan_body(ast, id, out)?;
            out.scopes.pop();
        }
        StmtKind::AssignField {
            receiver, value, ..
        }
        | StmtKind::CompoundAssignField {
            receiver, value, ..
        } => {
            visit_expr(ast, *receiver, out)?;
            visit_expr(ast, *value, out)?;
        }
        StmtKind::AssignIndex {
            receiver,
            index,
            value,
        }
        | StmtKind::CompoundAssignIndex {
            receiver,
            index,
            value,
            ..
        } => {
            out.grow(ast, *receiver)?;
            visit_expr(ast, *receiver, out)?;
            visit_expr(ast, *index, out)?;
            visit_expr(ast, *value, out)?;
        }
        StmtKind::ClassDecl { members, .. } => {
            for member in members {
                match member {
                    crate::ClassMember::Method { params, body, .. }
                    | crate::ClassMember::Constructor { params, body, .. } => {
                        scan_function(ast, params, crate::ArrowBody::Block(*body), out)?;
                    }
                    crate::ClassMember::Accessor { param, body, .. } => {
                        let params: Vec<_> = param.iter().map(|p| (**p).clone()).collect();
                        scan_function(ast, &params, crate::ArrowBody::Block(*body), out)?;
                    }
                    // An instance field's initializer runs at each `new`, as a
                    // constructor body does.
                    crate::ClassMember::Field { initializer, .. } => {
                        if let Some(init) = initializer {
                            scan_function(ast, &[], crate::ArrowBody::Expr(*init), out)?;
                        }
                    }
                }
            }
        }
        // Type space, control transfer with no operand, or lowered away before
        // inference (`lower_patterns`): nothing to walk.
        StmtKind::Break
        | StmtKind::Continue
        | StmtKind::LetPattern { .. }
        | StmtKind::ConstPattern { .. }
        | StmtKind::ForOfPattern { .. }
        | StmtKind::InterfaceDecl { .. }
        | StmtKind::EnumDecl { .. }
        | StmtKind::TypeAliasDecl { .. }
        | StmtKind::Import { .. }
        | StmtKind::ExportFrom { .. } => {}
    }
    let _: () = if extends_assignment {
        out.assignment_regions.pop();
    };
    Ok(())
}

/// Exhaustive for the same reason as [`visit_stmt`]: an arrow can
/// hide under any sub-expression.
fn visit_expr(ast: &Ast, id: ExprId, out: &mut Analysis) -> Result<(), CompilerFailure> {
    use crate::{ChainPart, ExprKind};
    let _: () = match &ast.try_expr(id).map_err(super::arena_failure)?.kind {
        ExprKind::FunctionExpression { name, function, .. } => {
            out.scopes.push(
                name.iter()
                    .map(|name| {
                        (
                            name.name.clone(),
                            Binding {
                                span: name.span,
                                function_depth: out.function_depth,
                                initialized: true,
                                block_local: false,
                                parameter: false,
                            },
                        )
                    })
                    .collect(),
            );
            visit_expr(ast, *function, out)?;
            out.scopes.pop();
        }
        ExprKind::Arrow { params, body, .. } => scan_function(ast, params, *body, out)?,
        // A compiler-written `undefined` reads no binding of that name.
        ExprKind::Identifier(_) if ast.synthetic_undefined.contains(&id) => {}
        ExprKind::Identifier(ident) => out.read(ident),
        // `x++` and `x--` write `x` exactly as `x = x + 1` does. `x!` is the
        // third `PostfixOp` and is a pure read — counting it would refuse
        // narrowing on every binding a closure merely asserts non-null.
        ExprKind::PostfixUnary { op, operand } => {
            if matches!(op, crate::PostfixOp::Inc | crate::PostfixOp::Dec)
                && let ExprKind::Identifier(ident) =
                    &ast.try_expr(*operand).map_err(super::arena_failure)?.kind
            {
                out.write(ident);
                out.write_arithmetic(ident);
            }
            visit_expr(ast, *operand, out)?;
        }
        ExprKind::Assign {
            target, op, value, ..
        } => {
            match &ast.try_expr(*target).map_err(super::arena_failure)?.kind {
                ExprKind::Identifier(ident) => {
                    out.read(ident);
                    out.write(ident);
                    out.grow(ast, *target)?;
                    if writes_back_arithmetic(ast, *op, *value)? {
                        out.write_arithmetic(ident);
                    }
                }
                ExprKind::IndexAccess { receiver, .. } => {
                    out.grow(ast, *receiver)?;
                    visit_expr(ast, *target, out)?;
                }
                _ => visit_expr(ast, *target, out)?,
            }
            visit_expr(ast, *value, out)?;
        }
        ExprKind::Binary { lhs, rhs, .. } => {
            visit_expr(ast, *lhs, out)?;
            visit_expr(ast, *rhs, out)?;
        }
        ExprKind::Unary { operand: inner, .. }
        | ExprKind::Void { operand: inner }
        | ExprKind::Typeof { operand: inner }
        | ExprKind::Delete { operand: inner }
        | ExprKind::As { expr: inner, .. }
        | ExprKind::InstanceOf { value: inner, .. }
        | ExprKind::Paren(inner)
        | ExprKind::FieldAccess {
            receiver: inner, ..
        } => {
            visit_expr(ast, *inner, out)?;
        }
        ExprKind::Call { callee, args, .. }
            if let Some(arrow) = super::iife::immediately_invoked_arrow(ast, *callee, args)? =>
        {
            // The body runs at the call, so its writes are the enclosing
            // function's own, as in TypeScript.
            let ExprKind::Arrow { params, body, .. } =
                &ast.try_expr(arrow).map_err(super::arena_failure)?.kind
            else {
                return Err(super::inference_failure(
                    "an immediately-invoked callee is an arrow",
                ));
            };
            scan_function_body(ast, params, *body, out)?;
        }
        ExprKind::Call { callee, args, .. } | ExprKind::New { callee, args, .. } => {
            if let ExprKind::FieldAccess { receiver, name } =
                &ast.try_expr(*callee).map_err(super::arena_failure)?.kind
                && matches!(name.name.as_str(), "push" | "unshift" | "splice" | "fill")
            {
                out.grow(ast, *receiver)?;
            }
            visit_call(ast, *callee, args, out)?;
        }
        ExprKind::ObjectLiteral { members } => {
            for m in members {
                for expression in m.expressions() {
                    visit_expr(ast, expression, out)?;
                }
            }
        }
        ExprKind::ArrayLiteral { elements } => {
            for e in elements {
                visit_expr(ast, e.value(), out)?;
            }
        }
        ExprKind::IndexAccess { receiver, index } => {
            visit_expr(ast, *receiver, out)?;
            visit_expr(ast, *index, out)?;
        }
        ExprKind::TemplateLiteral { exprs, .. } => {
            for &e in exprs {
                visit_expr(ast, e, out)?;
            }
        }
        ExprKind::Ternary { cond, then_, else_ } => {
            for &e in [cond, then_, else_] {
                visit_expr(ast, e, out)?;
            }
        }
        ExprKind::OptionalChain { base, parts } => {
            visit_expr(ast, *base, out)?;
            for part in parts {
                match part {
                    ChainPart::Index { idx, .. } => {
                        visit_expr(ast, *idx, out)?;
                    }
                    ChainPart::Call { args, .. } => {
                        for &a in args {
                            visit_expr(ast, a, out)?;
                        }
                    }
                    ChainPart::Field { .. } | ChainPart::NonNull { .. } => {}
                }
            }
        }
        // Leaves: no sub-expression to walk.
        ExprKind::Number(_)
        | ExprKind::BigInt(_)
        | ExprKind::String(_)
        | ExprKind::Boolean(_)
        | ExprKind::Null
        | ExprKind::This
        | ExprKind::ThisOutsideReceiver
        | ExprKind::Super
        | ExprKind::Regex { .. } => {}
    };
    Ok(())
}

fn visit_call(
    ast: &Ast,
    callee: ExprId,
    args: &[ExprId],
    out: &mut Analysis,
) -> Result<(), CompilerFailure> {
    if !is_direct_function(ast, callee)? {
        visit_expr(ast, callee, out)?;
        for &argument in args {
            visit_expr(ast, argument, out)?;
        }
        return Ok(());
    }
    for &argument in args {
        visit_expr(ast, argument, out)?;
    }
    let depth = out
        .function_depth
        .checked_add(1)
        .ok_or_else(|| super::inference_failure("binding analysis function depth overflow"))?;
    out.immediate_function_depths.push(depth);
    let result = visit_expr(ast, callee, out);
    out.immediate_function_depths.pop();
    result
}

fn is_direct_function(ast: &Ast, mut expression: ExprId) -> Result<bool, CompilerFailure> {
    for _ in 0..ast.exprs_len() {
        match &ast.try_expr(expression).map_err(super::arena_failure)?.kind {
            crate::ExprKind::Arrow { .. } | crate::ExprKind::FunctionExpression { .. } => {
                return Ok(true);
            }
            crate::ExprKind::Paren(inner) | crate::ExprKind::As { expr: inner, .. } => {
                expression = *inner;
            }
            _ => return Ok(false),
        }
    }
    Err(super::inference_failure(
        "cyclic expression while checking an immediate function call",
    ))
}

fn visit_iterable(
    ast: &Ast,
    loop_id: StmtId,
    iter: ExprId,
    out: &mut Analysis,
) -> Result<(), CompilerFailure> {
    let Some(bindings) = ast.for_of_pattern_bindings.get(&loop_id) else {
        visit_expr(ast, iter, out)?;
        return Ok(());
    };
    out.scopes.push(Default::default());
    for binding in bindings {
        out.declare(binding, false);
    }
    visit_expr(ast, iter, out)?;
    out.scopes.pop();

    Ok(())
}

impl Analysis {
    fn declare(&mut self, ident: &Ident, initialized: bool) {
        self.insert_binding(ident, initialized, false);
    }

    /// A `let`/`const` of the block, uninitialized until its statement runs.
    fn declare_block_local(&mut self, ident: &Ident) {
        self.insert_binding(ident, false, true);
    }

    fn insert_binding(&mut self, ident: &Ident, initialized: bool, block_local: bool) {
        let Some(scope) = self.scopes.last_mut() else {
            return;
        };
        if let Some(previous) = scope.get(&ident.name) {
            self.diagnostics.push(Diagnostic {
                severity: Severity::Error,
                span: ident.span,
                message: format!("binding `{}` is already declared in this scope", ident.name),
                help: vec![
                    "rename the binding or assign to the existing `let` instead".to_string(),
                ],
                notes: vec![(previous.span, "previously declared here".to_string())],
            });
            return;
        }
        scope.insert(
            ident.name.clone(),
            Binding {
                span: ident.span,
                function_depth: self.function_depth,
                initialized,
                block_local,
                parameter: false,
            },
        );
    }

    /// Declare a block's bindings before walking it. A `let`/`const` is
    /// uninitialized until its statement; a function declaration is hoisted,
    /// usable anywhere in the block.
    fn reserve_statements(&mut self, ast: &Ast, stmts: &[StmtId]) -> Result<(), CompilerFailure> {
        for &id in stmts {
            match &ast.try_stmt(id).map_err(super::arena_failure)?.kind {
                crate::StmtKind::Let { name, .. }
                | crate::StmtKind::Const { name, .. }
                | crate::StmtKind::ObjectRest { name, .. } => {
                    self.declare_block_local(name);
                }
                crate::StmtKind::Function { name, .. } => self.declare(name, true),
                _ => {}
            }
        }

        Ok(())
    }

    fn initialize(&mut self, ident: &Ident) {
        if let Some(binding) = self.scopes.last_mut().and_then(|s| s.get_mut(&ident.name)) {
            binding.initialized = true;
        }
    }

    /// The binding `name` resolves to, and the index of its scope.
    fn lookup(&self, name: &str) -> Option<(usize, &Binding)> {
        self.scopes
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, s)| s.get(name).map(|binding| (index, binding)))
    }

    /// The binding a use of `ident` resolves to, and the index of its scope. A
    /// `let`/`const` it names is captured by each nested function being scanned
    /// that is declared in the same block, which keeps the last declared of those.
    fn resolve_use(&mut self, ident: &Ident) -> Option<(usize, Binding)> {
        let (scope, binding) = self.lookup(&ident.name)?;
        let binding = *binding;
        // Parameters initialize before the body starts, so a nested function
        // never has to wait for a body declaration to capture their slots.
        if binding.block_local && !binding.parameter {
            self.note_capture(
                scope,
                Ident {
                    name: ident.name.clone(),
                    span: binding.span,
                },
            );
        }
        Some((scope, binding))
    }

    /// Whether the use is inside a nested function declared in block `scope`.
    /// `resolve_use` has recorded each such function's capture of the local
    /// (`note_capture` keeps the same functions), so its closure is created
    /// only once the local is declared, and calling it earlier is reported
    /// where it is called.
    fn is_inside_function_declared_in(&self, scope: usize) -> bool {
        self.nested_functions
            .iter()
            .any(|&(_, declaring_scope)| declaring_scope == scope)
    }

    fn note_capture(&mut self, scope: usize, local: Ident) {
        for &(function, declaring_scope) in &self.nested_functions {
            if declaring_scope != scope {
                continue;
            }
            self.nested_function_creation_points
                .entry(function)
                .and_modify(|last| {
                    if local.span.start > last.span.start {
                        *last = local.clone();
                    }
                })
                .or_insert_with(|| local.clone());
        }
    }

    fn read(&mut self, ident: &Ident) {
        let Some((scope, binding)) = self.resolve_use(ident) else {
            return;
        };
        if binding.initialized
            || self.is_inside_function_declared_in(scope)
            || (binding.parameter && self.is_deferred_from(binding.function_depth))
        {
            return;
        }
        let declaration = binding.span;
        self.diagnostics.push(Diagnostic {
            severity: Severity::Error,
            span: ident.span,
            message: format!("cannot access `{}` before its initialization", ident.name),
            help: vec!["move the declaration before this use, or rename the inner binding to refer to the outer one".to_string()],
            notes: vec![(declaration, "this declaration shadows outer bindings throughout the block".to_string())],
        });
    }

    /// Records that `receiver`, when it names a binding, is reassigned or gains
    /// elements.
    fn grow(&mut self, ast: &Ast, receiver: ExprId) -> Result<(), CompilerFailure> {
        if let crate::ExprKind::Identifier(ident) =
            &ast.try_expr(receiver).map_err(super::arena_failure)?.kind
        {
            self.grow_binding(ident);
        }
        Ok(())
    }

    fn grow_binding(&mut self, ident: &Ident) {
        if let Some((_, binding)) = self.lookup(&ident.name) {
            self.grown_bindings.insert(binding.span);
        }
    }

    fn write_arithmetic(&mut self, ident: &Ident) {
        match self.lookup(&ident.name) {
            Some((_, binding)) => {
                let declaration = binding.span;
                self.arithmetic_targets
                    .insert((ident.name.clone(), declaration));
            }
            None => {
                self.arithmetic_written_globals.insert(ident.name.clone());
            }
        }
    }

    fn is_deferred_from(&self, declaration_depth: usize) -> bool {
        (declaration_depth..self.function_depth)
            .filter_map(|depth| depth.checked_add(1))
            .any(|depth| !self.immediate_function_depths.contains(&depth))
    }

    fn write(&mut self, ident: &Ident) {
        let Some((_, binding)) = self.resolve_use(ident) else {
            if self.function_depth > 0 {
                self.function_written_globals.insert(ident.name.clone());
            }
            return;
        };
        let declaration = binding.span;
        if binding.function_depth < self.function_depth {
            self.mutators.insert((ident.name.clone(), declaration));
            return;
        }
        // A write in a branch or loop can execute after a closure elsewhere
        // in that statement. TypeScript extends its last-assignment position
        // to the containing statement, unless the binding was declared inside.
        let end = self
            .assignment_regions
            .iter()
            .filter(|span| span.start > declaration.start)
            .map(|span| span.end)
            .fold(ident.span.end, u32::max);
        self.last_assignments
            .entry(declaration)
            .and_modify(|last| *last = (*last).max(end))
            .or_insert(end);
    }
}

/// Whether a write of `value`, compound with `op` when `op` is set, is
/// arithmetic written back, which TypeScript checks against the base type of
/// the target's literal types: any compound arithmetic, and a plain `=` of an
/// operator binding at least as tightly as a shift.
fn writes_back_arithmetic(
    ast: &Ast,
    op: Option<crate::BinOp>,
    value: ExprId,
) -> Result<bool, CompilerFailure> {
    use crate::BinOp::*;
    use crate::ExprKind;
    if let Some(op) = op {
        return Ok(!matches!(op, And | Or | NullishCoalesce));
    }
    let mut value = value;
    loop {
        match &ast.try_expr(value).map_err(super::arena_failure)?.kind {
            ExprKind::Paren(inner) => value = *inner,
            ExprKind::Binary { op, .. } => {
                return Ok(matches!(
                    op,
                    Add | Sub | Mul | Div | Rem | Pow | Shl | Shr | UnsignedShr
                ));
            }
            _ => return Ok(false),
        }
    }
}

fn scan_body(ast: &Ast, body: StmtId, out: &mut Analysis) -> Result<(), CompilerFailure> {
    let crate::StmtKind::Block(stmts) = &ast.try_stmt(body).map_err(super::arena_failure)?.kind
    else {
        visit_stmt(ast, body, out)?;
        return Ok(());
    };
    out.reserve_statements(ast, stmts)?;
    for &id in stmts {
        visit_stmt(ast, id, out)?;
    }

    Ok(())
}

fn scan_function(
    ast: &Ast,
    params: &[crate::ParamDecl],
    body: crate::ArrowBody,
    out: &mut Analysis,
) -> Result<(), CompilerFailure> {
    out.function_depth = out
        .function_depth
        .checked_add(1)
        .ok_or_else(|| super::inference_failure("binding analysis function depth overflow"))?;
    scan_function_body(ast, params, body, out)?;
    out.function_depth -= 1;
    Ok(())
}

/// A function's parameters and body, at the current function depth.
fn scan_function_body(
    ast: &Ast,
    params: &[crate::ParamDecl],
    body: crate::ArrowBody,
    out: &mut Analysis,
) -> Result<(), CompilerFailure> {
    let mut scope = BTreeMap::new();
    // Duplicate parameters have a dedicated diagnostic during signature resolution.
    for param in params {
        scope.entry(param.name.name.clone()).or_insert(Binding {
            span: param.name.span,
            function_depth: out.function_depth,
            initialized: false,
            block_local: true,
            parameter: true,
        });
    }
    out.scopes.push(scope);
    for param in params {
        if let Some(statements) = ast.parameter_bindings.get(&param.name.span) {
            out.reserve_statements(ast, statements)?;
        }
    }
    if let Some(scope) = out.scopes.last_mut() {
        for binding in scope.values_mut() {
            binding.parameter = true;
        }
    }
    for param in params {
        if let Some(default) = param.default {
            visit_expr(ast, default, out)?;
        }
        out.initialize(&param.name);
        if let Some(statements) = ast.parameter_bindings.get(&param.name.span) {
            for &statement in statements {
                visit_stmt(ast, statement, out)?;
            }
        }
    }
    match body {
        crate::ArrowBody::Expr(expr) => visit_expr(ast, expr, out)?,
        crate::ArrowBody::Block(body) => scan_body(ast, body, out)?,
    }
    out.scopes.pop();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::analyze;

    fn analyze_source(source: &str) -> super::Analysis {
        let file = crate::FileId(0);
        let mut asi = crate::Asi::new(source, file);
        let mut tokens = Vec::new();
        loop {
            let token = asi.next_token();
            let done = matches!(token.kind, crate::TokenKind::Eof);
            tokens.push(token);
            if done {
                break;
            }
        }
        assert!(asi.finish().unwrap().is_empty());
        let (ast, diagnostics) = crate::parser::parse_checked(source, tokens, file).unwrap();
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        analyze(&crate::lower_patterns::lower(ast).unwrap()).unwrap()
    }

    #[test]
    fn parameter_defaults_observe_initialization_order() {
        for source in [
            "function f(a: number = b, b: number = 1): void {}",
            "function f(a: number = a): void {}",
        ] {
            let analysis = analyze_source(source);
            assert!(
                analysis
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.message.contains("before its initialization")),
                "{source}: {:?}",
                analysis.diagnostics
            );
        }
        let analysis = analyze_source("function f(a: number = 1, b: number = a): void {}");
        assert!(
            analysis.diagnostics.is_empty(),
            "{:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn default_closures_can_capture_later_parameters() {
        for source in [
            "function f(read: () => number = () => value, value: number = 5): number { return read(); }",
            "function f(read: () => number = () => value, { value }: { value: number } = { value: 5 }): number { return read(); }",
        ] {
            let analysis = analyze_source(source);
            assert!(
                analysis.diagnostics.is_empty(),
                "{source}: {:?}",
                analysis.diagnostics
            );
        }
    }

    #[test]
    fn nested_functions_can_capture_initialized_parameters() {
        for source in [
            "function outer(value: number): number { function read(): number { return value; } return read(); }",
            "function outer(value: number = 1): number { function read(): number { return value; } return read(); }",
            "function outer({ value }: { value: number }): number { function read(): number { return value; } return read(); }",
        ] {
            let analysis = analyze_source(source);
            assert!(
                analysis.diagnostics.is_empty(),
                "{source}: {:?}",
                analysis.diagnostics
            );
            assert!(
                analysis.nested_function_creation_points.is_empty(),
                "{source}"
            );
        }

        let analysis = analyze_source(
            "function outer(): number { const value = 1; function read(): number { return value; } return read(); }",
        );
        assert_eq!(analysis.nested_function_creation_points.len(), 1);
    }

    #[test]
    fn immediate_default_closures_observe_parameter_tdz() {
        for source in [
            "function f(value: number = (() => later)(), later: number = 5): number { return value; }",
            "function f(value: number = (function(): number { return later; })(), later: number = 5): number { return value; }",
            "function f(value: number = (() => (() => later)())(), later: number = 5): number { return value; }",
        ] {
            let analysis = analyze_source(source);
            assert!(
                analysis
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.message.contains("before its initialization")),
                "{source}: {:?}",
                analysis.diagnostics
            );
        }
        let analysis = analyze_source(
            "function f(read: () => number = () => (() => later)(), later: number = 5): number { return read(); }",
        );
        assert!(
            analysis.diagnostics.is_empty(),
            "{:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn void_in_default_preserves_closure_write_analysis() {
        let analysis = analyze_source(
            "function outer(): void { let x: number | null = null; const f = (a: unknown = void (x = 1)) => a; }",
        );
        assert!(
            analysis.diagnostics.is_empty(),
            "{:?}",
            analysis.diagnostics
        );
        assert!(analysis.mutators.iter().any(|(name, _)| name == "x"));
    }
}
