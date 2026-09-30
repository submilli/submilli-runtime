//! Lexical binding checks and closure-write analysis before inference.
//! Declaration spans remain stable when inference revisits a loop or generic body.

use crate::compiler_error::CompilerFailure;

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::{Ast, Diagnostic, ExprId, Ident, Severity, Span, StmtId};

#[derive(Default)]
pub(super) struct Analysis {
    pub(super) mutators: HashSet<(String, Span)>,
    pub(super) last_assignments: HashMap<Span, u32>,
    /// Nested function declarations, by name span, whose bodies read or write a
    /// `let`/`const` of the block they are declared in, with the last declared
    /// of those. Their closure can exist only once it is declared; any other is
    /// hoisted to the block's start.
    pub(super) nested_function_creation_points: HashMap<Span, Ident>,
    pub(super) diagnostics: Vec<Diagnostic>,
    scopes: Vec<BTreeMap<String, Binding>>,
    function_depth: usize,
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
    /// Declared by a `let`/`const` statement, so it has no value before that
    /// statement runs. Parameters and hoisted functions have one from the start.
    block_local: bool,
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
        StmtKind::Assign { target, value } | StmtKind::CompoundAssign { target, value, .. } => {
            out.read(target);
            out.write(target);
            visit_expr(ast, *value, out)?;
        }
        StmtKind::Let { name, value, .. } | StmtKind::Const { name, value, .. } => {
            visit_expr(ast, *value, out)?;
            out.initialize(name);
            out.write(name);
        }
        StmtKind::ConstRest { name, source, .. } => {
            visit_expr(ast, *source, out)?;
            out.initialize(name);
        }
        StmtKind::Function {
            name, params, body, ..
        } => {
            // At the top level there is no scope, and the function is no closure.
            let declaring_scope = out.scopes.len().checked_sub(1);
            if declaring_scope.is_some() && out.function_depth == 0 {
                out.reject_top_level_block_function(name);
            }
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
            for case in cases {
                for &value in &case.values {
                    visit_expr(ast, value, out)?;
                }
                visit_stmt(ast, case.body, out)?;
            }
            if let Some(d) = default {
                visit_stmt(ast, d.body, out)?;
            }
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
                    crate::ClassMember::Field { initializer, .. } => {
                        if let Some(init) = initializer {
                            visit_expr(ast, *init, out)?;
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
                            },
                        )
                    })
                    .collect(),
            );
            visit_expr(ast, *function, out)?;
            out.scopes.pop();
        }
        ExprKind::Arrow { params, body, .. } => scan_function(ast, params, *body, out)?,
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
            }
            visit_expr(ast, *operand, out)?;
        }
        ExprKind::Assign { target, value, .. } => {
            if let ExprKind::Identifier(ident) =
                &ast.try_expr(*target).map_err(super::arena_failure)?.kind
            {
                out.read(ident);
                out.write(ident);
            } else {
                visit_expr(ast, *target, out)?;
            }
            visit_expr(ast, *value, out)?;
        }
        ExprKind::Binary { lhs, rhs, .. } => {
            visit_expr(ast, *lhs, out)?;
            visit_expr(ast, *rhs, out)?;
        }
        ExprKind::Unary { operand: inner, .. }
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
        ExprKind::Call { callee, args, .. } | ExprKind::New { callee, args, .. } => {
            visit_expr(ast, *callee, out)?;
            for &a in args {
                visit_expr(ast, a, out)?;
            }
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
                | crate::StmtKind::ConstRest { name, .. } => {
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

    /// The binding a use of `ident` resolves to. A `let`/`const` it names is
    /// captured by each nested function being scanned that is declared in the
    /// same block, which keeps the last declared of those.
    fn resolve_use(&mut self, ident: &Ident) -> Option<Binding> {
        let (scope, binding) = self.lookup(&ident.name)?;
        let binding = *binding;
        if binding.block_local {
            self.note_capture(
                scope,
                Ident {
                    name: ident.name.clone(),
                    span: binding.span,
                },
            );
        }
        Some(binding)
    }

    /// A closure in a top-level block can't yet capture that block's bindings
    /// (SUB-1070), which a function declared there nearly always needs, even
    /// just to call itself.
    fn reject_top_level_block_function(&mut self, name: &Ident) {
        self.diagnostics.push(Diagnostic {
            severity: Severity::Error,
            span: name.span,
            message: "a function can't be declared in a top-level block yet".to_string(),
            help: vec![
                "declare it at the top level of the module, or inside a function".to_string(),
            ],
            notes: Vec::new(),
        });
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
        let Some(binding) = self.resolve_use(ident) else {
            return;
        };
        if binding.initialized {
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

    fn write(&mut self, ident: &Ident) {
        let Some(binding) = self.resolve_use(ident) else {
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
    let mut scope = BTreeMap::new();
    // Duplicate parameters have a dedicated diagnostic during signature resolution.
    for param in params {
        scope.entry(param.name.name.clone()).or_insert(Binding {
            span: param.name.span,
            function_depth: out.function_depth,
            initialized: true,
            block_local: false,
        });
    }
    out.scopes.push(scope);
    match body {
        crate::ArrowBody::Expr(expr) => visit_expr(ast, expr, out)?,
        crate::ArrowBody::Block(body) => scan_body(ast, body, out)?,
    }
    out.scopes.pop();
    out.function_depth -= 1;

    Ok(())
}
