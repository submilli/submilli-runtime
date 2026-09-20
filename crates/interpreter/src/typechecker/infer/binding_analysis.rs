//! Lexical binding checks and closure-write analysis before inference.
//! Declaration spans remain stable when inference revisits a loop or generic body.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::{Ast, Diagnostic, ExprId, Ident, Severity, Span, StmtId};

#[derive(Default)]
pub(super) struct Analysis {
    pub(super) mutators: HashSet<(String, Span)>,
    pub(super) last_assignments: HashMap<Span, u32>,
    pub(super) diagnostics: Vec<Diagnostic>,
    scopes: Vec<BTreeMap<String, Binding>>,
    function_depth: usize,
    assignment_regions: Vec<Span>,
}

struct Binding {
    span: Span,
    function_depth: usize,
    initialized: bool,
}

pub(super) fn analyze(ast: &Ast) -> Analysis {
    let mut analysis = Analysis::default();
    for &id in &ast.top_level {
        visit_stmt(ast, id, &mut analysis);
    }
    analysis
}

/// No `_` arm: a statement kind that stops the walk hides every arrow below it,
/// and the resulting narrowing is unsound rather than merely imprecise — the
/// enclosing frame reads a stale shadow while the closure has already written
/// the slot. A new statement kind should fail the build here, not go unscanned.
fn visit_stmt(ast: &Ast, id: StmtId, out: &mut Analysis) {
    use crate::StmtKind;
    let extends_assignment = matches!(
        ast.stmt(id).kind,
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
        out.assignment_regions.push(ast.stmt(id).span);
    }
    match &ast.stmt(id).kind {
        StmtKind::Assign { target, value } | StmtKind::CompoundAssign { target, value, .. } => {
            out.read(target);
            out.write(target);
            visit_expr(ast, *value, out);
        }
        StmtKind::Let { name, value, .. } | StmtKind::Const { name, value, .. } => {
            visit_expr(ast, *value, out);
            out.initialize(name);
            out.write(name);
        }
        StmtKind::ConstRest { name, source, .. } => {
            visit_expr(ast, *source, out);
            out.initialize(name);
        }
        StmtKind::Function { params, body, .. } => {
            scan_function(ast, params, crate::ArrowBody::Block(*body), out);
        }
        StmtKind::If {
            condition,
            then_block,
            else_block,
        } => {
            visit_expr(ast, *condition, out);
            visit_stmt(ast, *then_block, out);
            if let Some(e) = else_block {
                visit_stmt(ast, *e, out);
            }
        }
        StmtKind::While { condition, body } | StmtKind::DoWhile { body, condition } => {
            visit_expr(ast, *condition, out);
            visit_stmt(ast, *body, out);
        }
        StmtKind::For {
            init,
            condition,
            update,
            body,
        } => {
            out.scopes.push(Default::default());
            if let Some(init) = init {
                out.reserve_statements(ast, &[*init]);
            }
            for s in [init, update].into_iter().flatten() {
                visit_stmt(ast, *s, out);
            }
            if let Some(c) = condition {
                visit_expr(ast, *c, out);
            }
            visit_stmt(ast, *body, out);
            out.scopes.pop();
        }
        StmtKind::ForOf {
            name, iter, body, ..
        } => {
            out.scopes.push(Default::default());
            out.declare(name, false);
            visit_iterable(ast, id, *iter, out);
            out.initialize(name);
            visit_stmt(ast, *body, out);
            out.scopes.pop();
        }
        StmtKind::Switch {
            discriminant,
            cases,
            default,
        } => {
            visit_expr(ast, *discriminant, out);
            for case in cases {
                for &value in &case.values {
                    visit_expr(ast, value, out);
                }
                visit_stmt(ast, case.body, out);
            }
            if let Some(d) = default {
                visit_stmt(ast, d.body, out);
            }
        }
        StmtKind::Try {
            body,
            catches,
            finally,
        } => {
            visit_stmt(ast, *body, out);
            for clause in catches {
                out.scopes.push(Default::default());
                out.declare(&clause.binding, true);
                scan_body(ast, clause.body, out);
                out.scopes.pop();
            }
            if let Some(f) = finally {
                visit_stmt(ast, *f, out);
            }
        }
        StmtKind::Return(value) => {
            if let Some(v) = value {
                visit_expr(ast, *v, out);
            }
        }
        StmtKind::Throw { value } | StmtKind::Expr(value) => {
            visit_expr(ast, *value, out);
        }
        StmtKind::Block(_) => {
            out.scopes.push(Default::default());
            scan_body(ast, id, out);
            out.scopes.pop();
        }
        StmtKind::AssignField {
            receiver, value, ..
        }
        | StmtKind::CompoundAssignField {
            receiver, value, ..
        } => {
            visit_expr(ast, *receiver, out);
            visit_expr(ast, *value, out);
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
            visit_expr(ast, *receiver, out);
            visit_expr(ast, *index, out);
            visit_expr(ast, *value, out);
        }
        StmtKind::ClassDecl { members, .. } => {
            for member in members {
                match member {
                    crate::ClassMember::Method { params, body, .. }
                    | crate::ClassMember::Constructor { params, body, .. } => {
                        scan_function(ast, params, crate::ArrowBody::Block(*body), out);
                    }
                    crate::ClassMember::Accessor { param, body, .. } => {
                        let params: Vec<_> = param.iter().map(|p| (**p).clone()).collect();
                        scan_function(ast, &params, crate::ArrowBody::Block(*body), out);
                    }
                    crate::ClassMember::Field { initializer, .. } => {
                        if let Some(init) = initializer {
                            visit_expr(ast, *init, out);
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
    if extends_assignment {
        out.assignment_regions.pop();
    }
}

/// Exhaustive for the same reason as [`visit_stmt`]: an arrow can
/// hide under any sub-expression.
fn visit_expr(ast: &Ast, id: ExprId, out: &mut Analysis) {
    use crate::{ChainPart, ExprKind};
    match &ast.expr(id).kind {
        ExprKind::Arrow { params, body, .. } => scan_function(ast, params, *body, out),
        ExprKind::Identifier(ident) => out.read(ident),
        // `x++` and `x--` write `x` exactly as `x = x + 1` does. `x!` is the
        // third `PostfixOp` and is a pure read — counting it would refuse
        // narrowing on every binding a closure merely asserts non-null.
        ExprKind::PostfixUnary { op, operand } => {
            if matches!(op, crate::PostfixOp::Inc | crate::PostfixOp::Dec)
                && let ExprKind::Identifier(ident) = &ast.expr(*operand).kind
            {
                out.write(ident);
            }
            visit_expr(ast, *operand, out);
        }
        ExprKind::Binary { lhs, rhs, .. } => {
            visit_expr(ast, *lhs, out);
            visit_expr(ast, *rhs, out);
        }
        ExprKind::Unary { operand: inner, .. }
        | ExprKind::Typeof { operand: inner }
        | ExprKind::As { expr: inner, .. }
        | ExprKind::InstanceOf { value: inner, .. }
        | ExprKind::Paren(inner)
        | ExprKind::FieldAccess {
            receiver: inner, ..
        } => {
            visit_expr(ast, *inner, out);
        }
        ExprKind::Call { callee, args, .. } | ExprKind::New { callee, args, .. } => {
            visit_expr(ast, *callee, out);
            for &a in args {
                visit_expr(ast, a, out);
            }
        }
        ExprKind::ObjectLiteral { members } => {
            for m in members {
                visit_expr(ast, m.value(), out);
            }
        }
        ExprKind::ArrayLiteral { elements } => {
            for e in elements {
                visit_expr(ast, e.value(), out);
            }
        }
        ExprKind::IndexAccess { receiver, index } => {
            visit_expr(ast, *receiver, out);
            visit_expr(ast, *index, out);
        }
        ExprKind::TemplateLiteral { exprs, .. } => {
            for &e in exprs {
                visit_expr(ast, e, out);
            }
        }
        ExprKind::Ternary { cond, then_, else_ } => {
            for &e in [cond, then_, else_] {
                visit_expr(ast, e, out);
            }
        }
        ExprKind::OptionalChain { base, parts } => {
            visit_expr(ast, *base, out);
            for part in parts {
                match part {
                    ChainPart::Index { idx, .. } => {
                        visit_expr(ast, *idx, out);
                    }
                    ChainPart::Call { args, .. } => {
                        for &a in args {
                            visit_expr(ast, a, out);
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
        | ExprKind::Super
        | ExprKind::Regex { .. } => {}
    }
}

fn visit_iterable(ast: &Ast, loop_id: StmtId, iter: ExprId, out: &mut Analysis) {
    let Some(bindings) = ast.for_of_pattern_bindings.get(&loop_id) else {
        visit_expr(ast, iter, out);
        return;
    };
    out.scopes.push(Default::default());
    for binding in bindings {
        out.declare(binding, false);
    }
    visit_expr(ast, iter, out);
    out.scopes.pop();
}

impl Analysis {
    fn declare(&mut self, ident: &Ident, initialized: bool) {
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
            },
        );
    }

    fn reserve_statements(&mut self, ast: &Ast, stmts: &[StmtId]) {
        for &id in stmts {
            if let crate::StmtKind::Let { name, .. }
            | crate::StmtKind::Const { name, .. }
            | crate::StmtKind::ConstRest { name, .. } = &ast.stmt(id).kind
            {
                self.declare(name, false);
            }
        }
    }

    fn initialize(&mut self, ident: &Ident) {
        if let Some(binding) = self.scopes.last_mut().and_then(|s| s.get_mut(&ident.name)) {
            binding.initialized = true;
        }
    }

    fn lookup(&self, name: &str) -> Option<&Binding> {
        self.scopes.iter().rev().find_map(|s| s.get(name))
    }

    fn read(&mut self, ident: &Ident) {
        let Some(binding) = self.lookup(&ident.name) else {
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
        let Some(binding) = self.lookup(&ident.name) else {
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

fn scan_body(ast: &Ast, body: StmtId, out: &mut Analysis) {
    let crate::StmtKind::Block(stmts) = &ast.stmt(body).kind else {
        visit_stmt(ast, body, out);
        return;
    };
    out.reserve_statements(ast, stmts);
    for &id in stmts {
        visit_stmt(ast, id, out);
    }
}

fn scan_function(
    ast: &Ast,
    params: &[crate::ParamDecl],
    body: crate::ArrowBody,
    out: &mut Analysis,
) {
    out.function_depth += 1;
    out.scopes.push(Default::default());
    // Duplicate parameters have a dedicated diagnostic during signature resolution.
    for param in params {
        out.scopes
            .last_mut()
            .expect("function scope")
            .entry(param.name.name.clone())
            .or_insert(Binding {
                span: param.name.span,
                function_depth: out.function_depth,
                initialized: true,
            });
    }
    match body {
        crate::ArrowBody::Expr(expr) => visit_expr(ast, expr, out),
        crate::ArrowBody::Block(body) => scan_body(ast, body, out),
    }
    out.scopes.pop();
    out.function_depth -= 1;
}
