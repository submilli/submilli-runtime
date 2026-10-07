//! Structured typed-AST lowering to a bounded graph. Continuations carry every
//! completion separately, so finally runs on returns, loop jumps and throws.

use super::*;
use crate::typechecker::infer::narrowing::{BindingId, PathElem};
use crate::{BinOp, TypedArrayElement, TypedCatchClause, TypedExpr};

const MAX_DEPTH: usize = 96;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ThrowKind {
    Denied,
    Unknown,
}

#[derive(Clone)]
pub(super) enum Action {
    None,
    Exit,
    ThrowExit(ThrowKind),
    Check {
        name: Option<String>,
        roots: BTreeSet<String>,
    },
    Invalidate(Option<String>),
    Iteration,
    Coverage,
}

#[derive(Clone)]
pub(super) struct Call {
    pub target: usize,
    pub witness: AuthorityWitnessStep,
    pub bindings: BTreeMap<String, BTreeSet<String>>,
}

pub(super) struct FlowNode {
    pub action: Action,
    pub span: Span,
    pub description: Option<String>,
    pub normal: Vec<usize>,
    pub exceptional: BTreeMap<ThrowKind, Vec<usize>>,
    pub throws: BTreeSet<ThrowKind>,
    pub effects: Vec<AuthorityEffect>,
    pub calls: Vec<Call>,
    pub item_scopes: Vec<Vec<String>>,
    pub lookup: bool,
}

pub(super) struct FlowGraph {
    pub entry: usize,
    pub nodes: Vec<FlowNode>,
}

#[derive(Clone)]
struct Continuations {
    normal: usize,
    returned: usize,
    broken: usize,
    continued: usize,
    thrown: BTreeMap<ThrowKind, Vec<usize>>,
}

pub(super) fn build(
    builder: &Builder<'_>,
    work: &mut usize,
) -> Result<Vec<FlowGraph>, CompilerFailure> {
    let mut operations: Vec<BTreeMap<(u32, u32, u32), Operation>> =
        vec![BTreeMap::new(); builder.nodes.len()];
    for (caller, node) in builder.nodes.iter().enumerate() {
        for effect in &node.callable.direct_effects {
            if let Some(span) = builder.effect_spans.get(effect) {
                operations[caller]
                    .entry(span_key(*span))
                    .or_default()
                    .effects
                    .push(effect.clone());
            }
        }
    }
    for edge in &builder.edges {
        if let Some(target) = edge.target {
            operations[edge.caller]
                .entry(span_key(edge.raw_span))
                .or_default()
                .calls
                .push(Call {
                    target,
                    witness: AuthorityWitnessStep {
                        caller: builder.nodes[edge.caller].callable.id.clone(),
                        target: builder.nodes[target].callable.id.clone(),
                        span: edge.span.clone(),
                    },
                    bindings: BTreeMap::new(),
                });
        }
    }
    let parameters = parameter_names(builder);
    let mut graphs = Vec::new();
    let mut total_nodes = 0;
    for (caller, callable) in builder.nodes.iter().enumerate() {
        let mut lower = Lower {
            builder,
            nodes: Vec::new(),
            work,
            total_nodes: &mut total_nodes,
            loops: Vec::new(),
            loop_depth: 0,
            lookup: false,
            aliases: BTreeMap::new(),
            lookup_roots: BTreeSet::new(),
            operations: std::mem::take(&mut operations[caller]),
            parameters: &parameters,
            represented: BTreeSet::new(),
            ambiguous: BTreeSet::new(),
        };
        let mut collector = CheckPayloads {
            values: Vec::new(),
            bindings: BTreeMap::new(),
            duplicates: BTreeSet::new(),
            declared: parameters
                .get(&caller)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .collect(),
        };
        body_walk::walk_roots(
            builder.ta,
            callable.root.statements.iter().copied(),
            callable.root.expressions.iter().copied(),
            &mut collector,
        )?;
        lower.aliases = collector.bindings;
        for duplicate in collector.duplicates {
            lower.aliases.remove(&duplicate);
            lower.ambiguous.insert(duplicate);
        }
        for payload in collector.values {
            let roots = lower.references(payload)?;
            lower.lookup_roots.extend(roots);
        }
        let exit = lower.node(
            Action::Exit,
            callable.raw_span,
            Vec::new(),
            &BTreeMap::new(),
            None,
        )?;
        let mut thrown = BTreeMap::new();
        for kind in [ThrowKind::Denied, ThrowKind::Unknown] {
            let index = lower.node(
                Action::ThrowExit(kind),
                callable.raw_span,
                Vec::new(),
                &BTreeMap::new(),
                None,
            )?;
            thrown.insert(kind, vec![index]);
        }
        let mut continuations = Continuations {
            normal: exit,
            returned: exit,
            broken: exit,
            continued: exit,
            thrown,
        };
        for expression in callable.root.expressions.iter().rev() {
            continuations.normal = lower.expr(*expression, &continuations, 0)?;
        }
        for statement in callable.root.statements.iter().rev() {
            continuations.normal = lower.stmt(*statement, &continuations, 0)?;
        }
        let mut gaps = BTreeSet::new();
        for (key, operation) in &lower.operations {
            if lower.represented.contains(key) {
                continue;
            }
            gaps.extend(operation.effects.iter().cloned());
            for call in &operation.calls {
                gaps.extend(
                    builder.nodes[call.target]
                        .callable
                        .transitive_effects
                        .iter()
                        .cloned(),
                );
            }
        }
        if !gaps.is_empty() {
            let gap = lower.node(
                Action::Coverage,
                callable.raw_span,
                vec![continuations.normal],
                &continuations.thrown,
                Some(
                    "effect discovery includes a route that control-flow analysis cannot model"
                        .into(),
                ),
            )?;
            lower.nodes[gap].effects = gaps.into_iter().collect();
            continuations.normal = gap;
        }
        graphs.push(FlowGraph {
            entry: continuations.normal,
            nodes: lower.nodes,
        });
    }
    Ok(graphs)
}

struct Lower<'a, 'b, 'c> {
    builder: &'a Builder<'b>,
    nodes: Vec<FlowNode>,
    work: &'c mut usize,
    total_nodes: &'c mut usize,
    /// Item binding and whole iterable identities for enclosing loops.
    loops: Vec<(String, BTreeSet<String>)>,
    loop_depth: usize,
    aliases: BTreeMap<String, ExprId>,
    lookup_roots: BTreeSet<String>,
    operations: BTreeMap<(u32, u32, u32), Operation>,
    parameters: &'c BTreeMap<usize, Vec<String>>,
    represented: BTreeSet<(u32, u32, u32)>,
    ambiguous: BTreeSet<String>,
    lookup: bool,
}

impl<'a, 'b, 'c> Lower<'a, 'b, 'c> {
    fn node(
        &mut self,
        action: Action,
        span: Span,
        normal: Vec<usize>,
        exceptional: &BTreeMap<ThrowKind, Vec<usize>>,
        description: Option<String>,
    ) -> Result<usize, CompilerFailure> {
        *self.total_nodes = checked_authority_budget(
            *self.total_nodes,
            1,
            self.builder.limits.edges,
            span,
            "control-flow node",
        )?;
        budget(self.builder, self.work, span, 1)?;
        let index = self.nodes.len();
        self.nodes.push(FlowNode {
            action,
            span,
            normal,
            exceptional: exceptional.clone(),
            description,
            throws: BTreeSet::new(),
            effects: Vec::new(),
            calls: Vec::new(),
            item_scopes: Vec::new(),
            lookup: false,
        });
        Ok(index)
    }

    fn depth(&self, depth: usize, span: Span) -> Result<(), CompilerFailure> {
        if depth >= MAX_DEPTH {
            return Err(limit(
                span,
                format!(
                    "authority guard analysis exceeds control-flow nesting limit of {MAX_DEPTH}"
                ),
            ));
        }
        Ok(())
    }

    fn step(
        &mut self,
        span: Span,
        next: usize,
        cont: &Continuations,
        description: &str,
    ) -> Result<usize, CompilerFailure> {
        self.node(
            Action::None,
            span,
            vec![next],
            &cont.thrown,
            Some(description.into()),
        )
    }

    fn stmt(
        &mut self,
        id: StmtId,
        cont: &Continuations,
        depth: usize,
    ) -> Result<usize, CompilerFailure> {
        let stmt = self
            .builder
            .ta
            .try_stmt(id)
            .map_err(crate::typechecker::arena_failure)?;
        self.depth(depth, stmt.span)?;
        let span = stmt.span;
        let depth = depth + 1;
        match &stmt.kind {
            TypedStmtKind::Block(statements) => self.statements(statements, cont, depth),
            TypedStmtKind::Expr(value) => self.expr(*value, cont, depth),
            TypedStmtKind::Let { name, value, .. } | TypedStmtKind::Const { name, value, .. } => {
                let old = self.lookup;
                self.lookup |= self.used_in_check(id)?;
                let invalidate = self.node(
                    Action::Invalidate(Some(name.name.clone())),
                    span,
                    vec![cont.normal],
                    &cont.thrown,
                    None,
                )?;
                let entry = self.expr(
                    *value,
                    &Continuations {
                        normal: invalidate,
                        ..cont.clone()
                    },
                    depth,
                );
                self.lookup = old;
                entry
            }
            TypedStmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                let then_entry = self.stmt(*then_block, cont, depth)?;
                let then_entry = self.step(
                    span,
                    then_entry,
                    cont,
                    "condition takes the checked or unchecked then branch",
                )?;
                let else_entry = if let Some(block) = else_block {
                    self.stmt(*block, cont, depth)?
                } else {
                    cont.normal
                };
                let else_entry = self.step(
                    span,
                    else_entry,
                    cont,
                    "condition takes the else branch, bypassing the then branch",
                )?;
                let branch = self.branch(*condition, then_entry, else_entry, cont)?;
                self.expr(
                    *condition,
                    &Continuations {
                        normal: branch,
                        ..cont.clone()
                    },
                    depth,
                )
            }
            TypedStmtKind::While { condition, body } => self.loop_body(
                None,
                Some(*condition),
                None,
                *body,
                false,
                cont,
                depth,
                span,
            ),
            TypedStmtKind::For {
                init,
                condition,
                update,
                body,
            } => {
                let entry =
                    self.loop_body(None, *condition, *update, *body, false, cont, depth, span)?;
                if let Some(init) = init {
                    self.stmt(
                        *init,
                        &Continuations {
                            normal: entry,
                            ..cont.clone()
                        },
                        depth,
                    )
                } else {
                    Ok(entry)
                }
            }
            TypedStmtKind::DoWhile { body, condition } => {
                self.loop_body(None, Some(*condition), None, *body, true, cont, depth, span)
            }
            TypedStmtKind::ForOf {
                name,
                iter,
                body,
                kind,
                ..
            } => {
                let collection = self.references(*iter)?;
                self.loops.push((name.name.clone(), collection));
                let entry = self.loop_body(
                    Some(&name.name),
                    None,
                    None,
                    *body,
                    false,
                    cont,
                    depth,
                    span,
                )?;
                self.loops.pop();
                let entry = if *kind == crate::typed_ast::ForOfKind::Array {
                    entry
                } else {
                    self.coverage(span, &Continuations { normal: entry, ..cont.clone() }, "iterator dispatch may execute unmodeled authority; deferred dispatch analysis is unproven")?
                };
                self.expr(
                    *iter,
                    &Continuations {
                        normal: entry,
                        ..cont.clone()
                    },
                    depth,
                )
            }
            TypedStmtKind::Break => {
                self.step(span, cont.broken, cont, "break exits this construct")
            }
            TypedStmtKind::Continue => self.step(
                span,
                cont.continued,
                cont,
                "continue begins the next iteration",
            ),
            TypedStmtKind::Return(value) => {
                let exit = self.step(span, cont.returned, cont, "return terminates this path")?;
                if let Some(value) = value {
                    self.expr(
                        *value,
                        &Continuations {
                            normal: exit,
                            ..cont.clone()
                        },
                        depth,
                    )
                } else {
                    Ok(exit)
                }
            }
            TypedStmtKind::Throw { value } => {
                let targets = cont.thrown[&ThrowKind::Unknown].clone();
                let exit = self.node(
                    Action::None,
                    span,
                    targets,
                    &cont.thrown,
                    Some("throw terminates or enters a catch handler".into()),
                )?;
                self.expr(
                    *value,
                    &Continuations {
                        normal: exit,
                        ..cont.clone()
                    },
                    depth,
                )
            }
            TypedStmtKind::Try {
                body,
                catches,
                finally,
            } => self.try_body(*body, catches, *finally, cont, depth, span),
            TypedStmtKind::Switch {
                discriminant,
                cases,
                default,
                ..
            } => {
                let case_cont = Continuations {
                    broken: cont.normal,
                    ..cont.clone()
                };
                let mut targets = Vec::new();
                for case in cases {
                    let entry = self.stmt(case.body, &case_cont, depth)?;
                    targets.push(self.step(case.span, entry, cont, "switch selects this case")?);
                }
                let fallback = if let Some(default) = default {
                    self.stmt(*default, &case_cont, depth)?
                } else {
                    cont.normal
                };
                targets.push(self.step(
                    span,
                    fallback,
                    cont,
                    "switch selects default or no matching case",
                )?);
                let dispatch = self.node(Action::None, span, targets, &cont.thrown, None)?;
                self.expr(
                    *discriminant,
                    &Continuations {
                        normal: dispatch,
                        ..cont.clone()
                    },
                    depth,
                )
            }
            TypedStmtKind::AssignLocal { ident, value, .. } => {
                let invalidate = self.node(
                    Action::Invalidate(Some(ident.name.clone())),
                    span,
                    vec![cont.normal],
                    &cont.thrown,
                    Some("assignment invalidates checks on the previous value".into()),
                )?;
                self.expr(
                    *value,
                    &Continuations {
                        normal: invalidate,
                        ..cont.clone()
                    },
                    depth,
                )
            }
            TypedStmtKind::AssignGlobal { value, .. } => {
                let invalidate = self.node(
                    Action::Invalidate(None),
                    span,
                    vec![cont.normal],
                    &cont.thrown,
                    None,
                )?;
                self.expr(
                    *value,
                    &Continuations {
                        normal: invalidate,
                        ..cont.clone()
                    },
                    depth,
                )
            }
            TypedStmtKind::AssignField {
                receiver,
                name,
                value,
            } => {
                let invalidate = self.node(
                    Action::Invalidate(None),
                    span,
                    vec![cont.normal],
                    &cont.thrown,
                    None,
                )?;
                let operation = self.operation(
                    name.span,
                    &[*receiver, *value],
                    &Continuations {
                        normal: invalidate,
                        ..cont.clone()
                    },
                    None,
                )?;
                self.expressions(
                    &[*receiver, *value],
                    &Continuations {
                        normal: operation,
                        ..cont.clone()
                    },
                    depth,
                )
            }
            TypedStmtKind::AssignIndex {
                receiver,
                index,
                value,
                ..
            } => {
                let invalidate = self.node(
                    Action::Invalidate(None),
                    span,
                    vec![cont.normal],
                    &cont.thrown,
                    None,
                )?;
                let bounds = self.throwing(
                    span,
                    &Continuations {
                        normal: invalidate,
                        ..cont.clone()
                    },
                )?;
                self.expressions(
                    &[*receiver, *index, *value],
                    &Continuations {
                        normal: bounds,
                        ..cont.clone()
                    },
                    depth,
                )
            }
            TypedStmtKind::NarrowRegion {
                source, path, body, ..
            } => {
                let entry = self.stmt(*body, cont, depth)?;
                if path.chain.is_empty() {
                    let narrowed = self.throwing(
                        span,
                        &Continuations {
                            normal: entry,
                            ..cont.clone()
                        },
                    )?;
                    self.expr(
                        *source,
                        &Continuations {
                            normal: narrowed,
                            ..cont.clone()
                        },
                        depth,
                    )
                } else {
                    Ok(entry)
                }
            }
            TypedStmtKind::ReboxLocal { .. } => Ok(cont.normal),
        }
    }

    fn statements(
        &mut self,
        statements: &[StmtId],
        cont: &Continuations,
        depth: usize,
    ) -> Result<usize, CompilerFailure> {
        let mut cont = cont.clone();
        for statement in statements.iter().rev() {
            cont.normal = self.stmt(*statement, &cont, depth)?;
        }
        Ok(cont.normal)
    }

    fn expressions(
        &mut self,
        values: &[ExprId],
        cont: &Continuations,
        depth: usize,
    ) -> Result<usize, CompilerFailure> {
        let mut cont = cont.clone();
        for value in values.iter().rev() {
            cont.normal = self.expr(*value, &cont, depth)?;
        }
        Ok(cont.normal)
    }

    #[allow(clippy::too_many_arguments)]
    fn loop_body(
        &mut self,
        binding: Option<&str>,
        condition: Option<ExprId>,
        update: Option<StmtId>,
        body: StmtId,
        do_first: bool,
        cont: &Continuations,
        depth: usize,
        span: Span,
    ) -> Result<usize, CompilerFailure> {
        let head = self.node(Action::None, span, Vec::new(), &cont.thrown, None)?;
        self.loop_depth += 1;
        let update = if let Some(update) = update {
            self.stmt(
                update,
                &Continuations {
                    normal: head,
                    ..cont.clone()
                },
                depth,
            )?
        } else {
            head
        };
        let body_cont = Continuations {
            normal: update,
            broken: cont.normal,
            continued: update,
            ..cont.clone()
        };
        let mut body_entry = self.stmt(body, &body_cont, depth)?;
        body_entry = self.node(
            Action::Iteration,
            span,
            vec![body_entry],
            &cont.thrown,
            Some("loop iteration invalidates prior element checks".into()),
        )?;
        if let Some(binding) = binding {
            body_entry = self.node(
                Action::Invalidate(Some(binding.into())),
                span,
                vec![body_entry],
                &cont.thrown,
                Some("new iteration cannot reuse a previous item check".into()),
            )?;
        }
        let dispatch = if let Some(condition) = condition {
            self.branch(condition, body_entry, cont.normal, cont)?
        } else {
            let successors = if binding.is_some() {
                vec![body_entry, cont.normal]
            } else {
                vec![body_entry]
            };
            self.node(
                Action::None,
                span,
                successors,
                &cont.thrown,
                Some("loop enters the body or finishes".into()),
            )?
        };
        let test = if let Some(condition) = condition {
            self.expr(
                condition,
                &Continuations {
                    normal: dispatch,
                    ..cont.clone()
                },
                depth,
            )?
        } else {
            dispatch
        };
        // `head` was allocated above in this same append-only graph.
        self.nodes[head].normal.push(test);
        self.loop_depth -= 1;
        Ok(if do_first { body_entry } else { head })
    }

    fn branch(
        &mut self,
        condition: ExprId,
        yes: usize,
        no: usize,
        cont: &Continuations,
    ) -> Result<usize, CompilerFailure> {
        let expression = self.expression(condition)?;
        let targets = match expression.kind {
            TypedExprKind::Boolean(true) => vec![yes],
            TypedExprKind::Boolean(false) => vec![no],
            _ => vec![yes, no],
        };
        self.node(Action::None, expression.span, targets, &cont.thrown, None)
    }

    fn try_body(
        &mut self,
        body: StmtId,
        catches: &[TypedCatchClause],
        finally: Option<StmtId>,
        cont: &Continuations,
        depth: usize,
        span: Span,
    ) -> Result<usize, CompilerFailure> {
        let mut wrapped = cont.clone();
        if let Some(finally) = finally {
            wrapped.normal = self.stmt(finally, cont, depth)?;
            wrapped.returned = self.stmt(
                finally,
                &Continuations {
                    normal: cont.returned,
                    ..cont.clone()
                },
                depth,
            )?;
            wrapped.broken = self.stmt(
                finally,
                &Continuations {
                    normal: cont.broken,
                    ..cont.clone()
                },
                depth,
            )?;
            wrapped.continued = self.stmt(
                finally,
                &Continuations {
                    normal: cont.continued,
                    ..cont.clone()
                },
                depth,
            )?;
            for kind in [ThrowKind::Denied, ThrowKind::Unknown] {
                let destination = self.node(
                    Action::None,
                    span,
                    cont.thrown[&kind].clone(),
                    &cont.thrown,
                    None,
                )?;
                let entry = self.stmt(
                    finally,
                    &Continuations {
                        normal: destination,
                        ..cont.clone()
                    },
                    depth,
                )?;
                wrapped.thrown.insert(kind, vec![entry]);
            }
        }
        let mut body_cont = wrapped.clone();
        for kind in [ThrowKind::Denied, ThrowKind::Unknown] {
            let mut destinations = Vec::new();
            let mut exhaustive = false;
            for catch in catches {
                let Type::ClassRef { mangled, .. } = catch.ty.peel() else {
                    continue;
                };
                let catches_all = *mangled == crate::mangle::prelude("Error");
                let catches_denied =
                    catches_all || *mangled == crate::mangle::prelude("PermissionDeniedError");
                if kind == ThrowKind::Denied && !catches_denied {
                    continue;
                }
                let entry = self.stmt(catch.body, &wrapped, depth)?;
                let entry = self.node(
                    Action::Invalidate(Some(catch.binding.name.clone())),
                    catch.span,
                    vec![entry],
                    &wrapped.thrown,
                    None,
                )?;
                destinations.push(self.step(
                    catch.span,
                    entry,
                    &wrapped,
                    "exception enters this catch without gaining authority",
                )?);
                if catches_all || kind == ThrowKind::Denied {
                    exhaustive = true;
                    break;
                }
            }
            if !exhaustive {
                destinations.extend(wrapped.thrown[&kind].iter().copied());
            }
            body_cont.thrown.insert(kind, destinations);
        }
        self.stmt(body, &body_cont, depth)
    }

    fn expr(
        &mut self,
        id: ExprId,
        cont: &Continuations,
        depth: usize,
    ) -> Result<usize, CompilerFailure> {
        let expression = self.expression(id)?;
        self.depth(depth, expression.span)?;
        let span = expression.span;
        let depth = depth + 1;
        match &expression.kind {
            TypedExprKind::Call { mangled, args, .. } => {
                self.call(mangled, args, span, cont, depth)
            }
            TypedExprKind::GenericCall { mangled, args, .. } => self.call(
                mangled,
                &args.iter().map(|arg| arg.expr).collect::<Vec<_>>(),
                span,
                cont,
                depth,
            ),
            TypedExprKind::Binary {
                op: BinOp::And | BinOp::Or | BinOp::NullishCoalesce,
                lhs,
                rhs,
            }
            | TypedExprKind::NullishCoalesce { lhs, rhs } => {
                let right = self.expr(*rhs, cont, depth)?;
                let split = self.node(
                    Action::None,
                    span,
                    vec![right, cont.normal],
                    &cont.thrown,
                    Some("short-circuit may bypass the right operand".into()),
                )?;
                self.expr(
                    *lhs,
                    &Continuations {
                        normal: split,
                        ..cont.clone()
                    },
                    depth,
                )
            }
            TypedExprKind::Binary { lhs, rhs, .. } => {
                let next = if self.expression(*lhs)?.ty.peel() == &Type::BigInt
                    || self.expression(*rhs)?.ty.peel() == &Type::BigInt
                {
                    self.throwing(span, cont)?
                } else {
                    cont.normal
                };
                self.expressions(
                    &[*lhs, *rhs],
                    &Continuations {
                        normal: next,
                        ..cont.clone()
                    },
                    depth,
                )
            }
            TypedExprKind::Ternary { cond, then_, else_ } => {
                let yes = self.expr(*then_, cont, depth)?;
                let no = self.expr(*else_, cont, depth)?;
                let split = self.branch(*cond, yes, no, cont)?;
                self.expr(
                    *cond,
                    &Continuations {
                        normal: split,
                        ..cont.clone()
                    },
                    depth,
                )
            }
            TypedExprKind::Sequence { stmts, result } => {
                let result = self.expr(*result, cont, depth)?;
                self.statements(
                    stmts,
                    &Continuations {
                        normal: result,
                        ..cont.clone()
                    },
                    depth,
                )
            }
            TypedExprKind::EffectThen { effect, result } => {
                self.expressions(&[*effect, *result], cont, depth)
            }
            TypedExprKind::Unary { operand, .. } => {
                let next = if self.expression(*operand)?.ty.peel() == &Type::BigInt {
                    self.throwing(span, cont)?
                } else {
                    cont.normal
                };
                self.expr(
                    *operand,
                    &Continuations {
                        normal: next,
                        ..cont.clone()
                    },
                    depth,
                )
            }
            TypedExprKind::TypeofTag { value, .. } | TypedExprKind::InstanceOf { value, .. } => {
                self.expr(*value, cont, depth)
            }
            TypedExprKind::Cast { value, check, .. } => {
                let next = if check.is_some() {
                    self.throwing(span, cont)?
                } else {
                    cont.normal
                };
                self.expr(
                    *value,
                    &Continuations {
                        normal: next,
                        ..cont.clone()
                    },
                    depth,
                )
            }
            TypedExprKind::NonNullAssert { value } => {
                let next = self.throwing(span, cont)?;
                self.expr(
                    *value,
                    &Continuations {
                        normal: next,
                        ..cont.clone()
                    },
                    depth,
                )
            }
            TypedExprKind::CallClosure { callee, args } => {
                let mut values = vec![*callee];
                values.extend(args);
                self.evaluated_operation(span, &values, cont, depth)
            }
            TypedExprKind::MethodCall { receiver, args, .. } => {
                let mut values = vec![*receiver];
                values.extend(args);
                self.evaluated_operation(span, &values, cont, depth)
            }
            TypedExprKind::GenericMethodCall { receiver, args, .. } => {
                let mut values = vec![*receiver];
                values.extend(args.iter().map(|arg| arg.expr));
                self.evaluated_operation(span, &values, cont, depth)
            }
            TypedExprKind::McpCall { args, .. }
            | TypedExprKind::SuperCtorCall { args, .. }
            | TypedExprKind::SuperMethodCall { args, .. }
            | TypedExprKind::IntrinsicCall { args, .. } => {
                self.evaluated_operation(span, args, cont, depth)
            }
            TypedExprKind::FieldAccess { receiver, .. }
            | TypedExprKind::InterfacePropertyAccess { receiver, .. } => {
                self.evaluated_operation(span, &[*receiver], cont, depth)
            }
            TypedExprKind::IndexAccess { receiver, index } => {
                let next = self.throwing(span, cont)?;
                self.expressions(
                    &[*receiver, *index],
                    &Continuations {
                        normal: next,
                        ..cont.clone()
                    },
                    depth,
                )
            }
            TypedExprKind::ObjectLiteral { members, .. } => {
                let values = members
                    .iter()
                    .flat_map(|member| member.expressions())
                    .collect::<Vec<_>>();
                self.expressions(&values, cont, depth)
            }
            TypedExprKind::ArrayLiteral { elements, .. } => self.expressions(
                &elements
                    .iter()
                    .map(TypedArrayElement::expr_id)
                    .collect::<Vec<_>>(),
                cont,
                depth,
            ),
            TypedExprKind::TupleLiteral { elements, .. } => self.expressions(elements, cont, depth),
            TypedExprKind::OptionalChain { base, parts } => {
                let mut next = cont.normal;
                for part in parts.iter().rev() {
                    let (values, part_span) = match part {
                        TypedChainPart::Call { args, span, .. }
                        | TypedChainPart::MethodCall { args, span, .. } => (args.clone(), *span),
                        TypedChainPart::Index { idx, span, .. } => (vec![*idx], *span),
                        TypedChainPart::Field { span, .. }
                        | TypedChainPart::InterfaceProperty { span, .. }
                        | TypedChainPart::NonNull { span, .. } => (Vec::new(), *span),
                    };
                    next = self.evaluated_operation(
                        part_span,
                        &values,
                        &Continuations {
                            normal: next,
                            ..cont.clone()
                        },
                        depth,
                    )?;
                    if part.is_optional() {
                        next = self.node(
                            Action::None,
                            part_span,
                            vec![next, cont.normal],
                            &cont.thrown,
                            Some("optional chain can bypass this operation".into()),
                        )?;
                    }
                }
                self.expr(
                    *base,
                    &Continuations {
                        normal: next,
                        ..cont.clone()
                    },
                    depth,
                )
            }
            TypedExprKind::Narrowed {
                source,
                inner,
                path,
                ..
            } => {
                let inner = self.expr(*inner, cont, depth)?;
                if path.chain.is_empty() {
                    let narrowed = self.throwing(
                        span,
                        &Continuations {
                            normal: inner,
                            ..cont.clone()
                        },
                    )?;
                    self.expr(
                        *source,
                        &Continuations {
                            normal: narrowed,
                            ..cont.clone()
                        },
                        depth,
                    )
                } else {
                    Ok(inner)
                }
            }
            TypedExprKind::PostfixUnary { target, .. } => {
                let (binding, values) = match target {
                    PostfixTarget::Local { ident, .. } => (Some(ident.name.clone()), Vec::new()),
                    PostfixTarget::Global { .. } => (None, Vec::new()),
                    PostfixTarget::Field { receiver, .. } => (None, vec![*receiver]),
                    PostfixTarget::Index {
                        receiver, index, ..
                    } => (None, vec![*receiver, *index]),
                };
                let invalidate = self.node(
                    Action::Invalidate(binding),
                    span,
                    vec![cont.normal],
                    &cont.thrown,
                    None,
                )?;
                self.evaluated_operation(
                    span,
                    &values,
                    &Continuations {
                        normal: invalidate,
                        ..cont.clone()
                    },
                    depth,
                )
            }
            TypedExprKind::LocalNarrowRef { .. } => self.throwing(span, cont),
            TypedExprKind::Closure { .. }
            | TypedExprKind::Number(_)
            | TypedExprKind::BigInt(_)
            | TypedExprKind::String(_)
            | TypedExprKind::Boolean(_)
            | TypedExprKind::Null
            | TypedExprKind::This
            | TypedExprKind::Regex { .. }
            | TypedExprKind::LocalRef { .. }
            | TypedExprKind::GlobalRef { .. }
            | TypedExprKind::FunctionRef { .. }
            | TypedExprKind::NumberEnumMember { .. }
            | TypedExprKind::StringEnumMember { .. } => Ok(cont.normal),
        }
    }

    fn call(
        &mut self,
        mangled: &MangledName,
        args: &[ExprId],
        span: Span,
        cont: &Continuations,
        depth: usize,
    ) -> Result<usize, CompilerFailure> {
        if crate::stdlib::security::is_check(mangled) {
            let name = args
                .first()
                .and_then(|id| self.builder.ta.try_expr(*id).ok())
                .and_then(|expression| match &expression.kind {
                    TypedExprKind::String(name) => Some(name.clone()),
                    _ => None,
                });
            let roots = if let Some(payload) = args.get(1) {
                self.references(*payload)?
            } else {
                BTreeSet::new()
            };
            let check = self.node(
                Action::Check { name, roots },
                span,
                vec![cont.normal],
                &cont.thrown,
                None,
            )?;
            self.nodes[check]
                .throws
                .extend([ThrowKind::Denied, ThrowKind::Unknown]);
            let before_check = if let Some(payload) = args.get(1)
                && self.payload_dispatch(*payload)?
            {
                self.coverage(span, &Continuations { normal: check, ..cont.clone() }, "check payload may dispatch user toJson code before authorization; deferred dispatch analysis is unproven")?
            } else {
                check
            };
            let old = self.lookup;
            self.lookup = true;
            let result = self.expressions(
                args,
                &Continuations {
                    normal: before_check,
                    ..cont.clone()
                },
                depth,
            );
            self.lookup = old;
            return result;
        }
        self.evaluated_operation(span, args, cont, depth)
    }

    fn coverage(
        &mut self,
        span: Span,
        cont: &Continuations,
        reason: &str,
    ) -> Result<usize, CompilerFailure> {
        let gap = self.node(
            Action::Coverage,
            span,
            vec![cont.normal],
            &cont.thrown,
            Some(reason.into()),
        )?;
        self.nodes[gap].effects.push(AuthorityEffect {
            capability: None,
            sink: source_span(self.builder.sources, span)?,
            known_bindings: BTreeMap::new(),
            unresolved: true,
            reason: Some(reason.into()),
        });
        self.nodes[gap].throws.insert(ThrowKind::Unknown);
        Ok(gap)
    }

    fn payload_dispatch(&mut self, payload: ExprId) -> Result<bool, CompilerFailure> {
        let expression = self.expression(payload)?;
        let mut types = vec![&expression.ty];
        while let Some(ty) = types.pop() {
            budget(self.builder, self.work, expression.span, 1)?;
            match ty.peel() {
                Type::Object { fields, index } => {
                    types.extend(fields.values().map(|field| &field.ty));
                    if let Some(index) = index {
                        types.push(&index.value);
                    }
                }
                Type::Array(element) => types.push(element),
                Type::Tuple(elements) | Type::Union(elements) => types.extend(elements),
                Type::Number
                | Type::NumberLiteral(_)
                | Type::BigInt
                | Type::String
                | Type::StringLiteral(_)
                | Type::Uint8Array
                | Type::Boolean
                | Type::BooleanLiteral(_)
                | Type::Null
                | Type::Void
                | Type::Never
                | Type::NumberEnum { .. }
                | Type::StringEnum { .. } => {}
                _ => return Ok(true),
            }
        }
        Ok(false)
    }

    fn throwing(&mut self, span: Span, cont: &Continuations) -> Result<usize, CompilerFailure> {
        let next = self.node(Action::None, span, vec![cont.normal], &cont.thrown, None)?;
        self.nodes[next].throws.insert(ThrowKind::Unknown);
        Ok(next)
    }

    fn evaluated_operation(
        &mut self,
        span: Span,
        values: &[ExprId],
        cont: &Continuations,
        depth: usize,
    ) -> Result<usize, CompilerFailure> {
        let operation = self.operation(span, values, cont, None)?;
        self.expressions(
            values,
            &Continuations {
                normal: operation,
                ..cont.clone()
            },
            depth,
        )
    }

    fn operation(
        &mut self,
        span: Span,
        values: &[ExprId],
        cont: &Continuations,
        description: Option<String>,
    ) -> Result<usize, CompilerFailure> {
        self.represented.insert(span_key(span));
        let mut operation = self
            .operations
            .get(&span_key(span))
            .cloned()
            .unwrap_or_default();
        for call in &mut operation.calls {
            if let Some(names) = self.parameters.get(&call.target) {
                let receiver = matches!(
                    self.builder.nodes[call.target].callable.kind,
                    AuthorityCallableKind::Method
                        | AuthorityCallableKind::Getter
                        | AuthorityCallableKind::Setter
                );
                for (name, value) in names.iter().zip(values.iter().skip(usize::from(receiver))) {
                    call.bindings.insert(name.clone(), self.references(*value)?);
                }
            }
        }
        let index = self.node(
            Action::None,
            span,
            vec![cont.normal],
            &cont.thrown,
            description,
        )?;
        let roots = values
            .iter()
            .map(|value| self.references(*value))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect::<BTreeSet<_>>();
        let mut item_scopes: Vec<Vec<String>> = self
            .loops
            .iter()
            .filter(|(item, _)| roots.iter().any(|root| reference_matches(root, item)))
            .map(|(item, collection)| {
                std::iter::once(item.clone())
                    .chain(collection.iter().cloned())
                    .collect()
            })
            .collect();
        if self.loop_depth > 0 {
            for root in roots.iter().filter(|root| root.ends_with("[]")) {
                item_scopes.push(vec![root.clone(), root.trim_end_matches("[]").to_string()]);
            }
        }
        let node = &mut self.nodes[index];
        node.effects = operation.effects;
        node.calls = operation.calls;
        node.item_scopes = item_scopes;
        node.lookup = self.lookup;
        if roots.iter().any(|root| root.starts_with("?shadowed"))
            || (self.loop_depth > 0 && roots.contains("?computed"))
        {
            node.action = Action::Coverage;
        }
        // Unresolved/external operations may return or throw. Locally resolved
        // calls instead use the least-fixed-point completion summary.
        if node.calls.is_empty() {
            node.throws.insert(ThrowKind::Unknown);
        }
        Ok(index)
    }

    fn expression(&self, id: ExprId) -> Result<&'b TypedExpr, CompilerFailure> {
        self.builder
            .ta
            .try_expr(id)
            .map_err(crate::typechecker::arena_failure)
    }

    fn references(&mut self, root: ExprId) -> Result<BTreeSet<String>, CompilerFailure> {
        let mut pending = vec![(root, false)];
        let mut seen = BTreeSet::new();
        let mut roots = BTreeSet::new();
        while let Some((id, indexed)) = pending.pop() {
            if !seen.insert((id, indexed)) {
                continue;
            }
            let expression = self.expression(id)?;
            budget(self.builder, self.work, expression.span, 1)?;
            let mut children = Vec::new();
            let mut reference = None;
            match &expression.kind {
                TypedExprKind::LocalRef { ident, .. } => {
                    reference = Some(if self.ambiguous.contains(&ident.name) {
                        format!("?shadowed:{}", ident.name)
                    } else {
                        ident.name.clone()
                    });
                    if let Some(alias) = self.aliases.get(&ident.name) {
                        pending.push((*alias, indexed));
                    }
                }
                TypedExprKind::GlobalRef { mangled, .. } => reference = Some(mangled.to_string()),
                TypedExprKind::LocalNarrowRef { path, .. } => {
                    reference = Some(match &path.root {
                        BindingId::Local { name, .. } if self.ambiguous.contains(name) => {
                            format!("?shadowed:{name}")
                        }
                        BindingId::Local { name, .. } => name.clone(),
                        BindingId::Global(mangled) => mangled.to_string(),
                        BindingId::This => "this".into(),
                    });
                    if path
                        .chain
                        .iter()
                        .any(|step| matches!(step, PathElem::Index(_) | PathElem::Key(..)))
                    {
                        reference = reference.map(|root| format!("{root}[]"));
                    }
                }
                TypedExprKind::This => reference = Some("this".into()),
                TypedExprKind::IndexAccess { receiver, index } => {
                    pending.push((*receiver, true));
                    pending.push((*index, false));
                }
                TypedExprKind::FieldAccess { receiver, .. }
                | TypedExprKind::InterfacePropertyAccess { receiver, .. } => {
                    children.push(*receiver);
                }
                TypedExprKind::ObjectLiteral { members, .. } => {
                    children.extend(members.iter().flat_map(|member| member.expressions()));
                }
                TypedExprKind::ArrayLiteral { elements, .. } => {
                    children.extend(elements.iter().map(TypedArrayElement::expr_id));
                }
                TypedExprKind::TupleLiteral { elements, .. } => children.extend(elements),
                TypedExprKind::Cast { value, .. } | TypedExprKind::NonNullAssert { value } => {
                    children.push(*value);
                }
                TypedExprKind::Narrowed { inner, .. } => children.push(*inner),
                TypedExprKind::EffectThen { result, .. }
                | TypedExprKind::Sequence { result, .. } => children.push(*result),
                TypedExprKind::Binary { lhs, rhs, .. }
                | TypedExprKind::NullishCoalesce { lhs, rhs } => children.extend([*lhs, *rhs]),
                TypedExprKind::Ternary { then_, else_, .. } => children.extend([*then_, *else_]),
                TypedExprKind::Call { args, .. } => {
                    children.extend(args);
                    roots.insert("?computed".into());
                }
                TypedExprKind::GenericCall { args, .. } => {
                    children.extend(args.iter().map(|arg| arg.expr));
                    roots.insert("?computed".into());
                }
                TypedExprKind::Number(_)
                | TypedExprKind::BigInt(_)
                | TypedExprKind::String(_)
                | TypedExprKind::Boolean(_)
                | TypedExprKind::Null
                | TypedExprKind::Regex { .. }
                | TypedExprKind::NumberEnumMember { .. }
                | TypedExprKind::StringEnumMember { .. } => {}
                // Unsupported value origins must not look like a root-free
                // literal payload: that would grant whole-route scope.
                TypedExprKind::FunctionRef { .. }
                | TypedExprKind::Unary { .. }
                | TypedExprKind::CallClosure { .. }
                | TypedExprKind::McpCall { .. }
                | TypedExprKind::IntrinsicCall { .. }
                | TypedExprKind::MethodCall { .. }
                | TypedExprKind::SuperCtorCall { .. }
                | TypedExprKind::SuperMethodCall { .. }
                | TypedExprKind::GenericMethodCall { .. }
                | TypedExprKind::Closure { .. }
                | TypedExprKind::TypeofTag { .. }
                | TypedExprKind::OptionalChain { .. }
                | TypedExprKind::PostfixUnary { .. }
                | TypedExprKind::InstanceOf { .. } => {
                    roots.insert("?computed".into());
                }
            }
            pending.extend(children.into_iter().map(|child| (child, indexed)));
            if let Some(reference) = reference {
                roots.insert(if indexed {
                    format!("{reference}[]")
                } else {
                    reference
                });
            }
        }
        Ok(roots)
    }

    fn used_in_check(&mut self, declaration: StmtId) -> Result<bool, CompilerFailure> {
        let stmt = self
            .builder
            .ta
            .try_stmt(declaration)
            .map_err(crate::typechecker::arena_failure)?;
        let name = match &stmt.kind {
            TypedStmtKind::Let { name, .. } | TypedStmtKind::Const { name, .. } => &name.name,
            _ => return Ok(false),
        };
        Ok(self.lookup_roots.contains(name))
    }
}

struct CheckPayloads {
    values: Vec<ExprId>,
    bindings: BTreeMap<String, ExprId>,
    duplicates: BTreeSet<String>,
    declared: BTreeSet<String>,
}
impl Visitor for CheckPayloads {
    fn descend_into_closures(&self) -> bool {
        false
    }
    fn visit_stmt(&mut self, kind: &TypedStmtKind) -> Result<(), CompilerFailure> {
        if let TypedStmtKind::Try { catches, .. } = kind {
            for catch in catches {
                if !self.declared.insert(catch.binding.name.clone()) {
                    self.duplicates.insert(catch.binding.name.clone());
                }
            }
        }
        let binding = match kind {
            TypedStmtKind::Const { name, .. }
            | TypedStmtKind::Let { name, .. }
            | TypedStmtKind::ForOf { name, .. } => Some(name),
            _ => None,
        };
        if let Some(name) = binding
            && !self.declared.insert(name.name.clone())
        {
            self.duplicates.insert(name.name.clone());
        }
        if let TypedStmtKind::Const { name, value, .. } = kind {
            self.bindings.insert(name.name.clone(), *value);
        }
        Ok(())
    }
    fn visit_expr(&mut self, _id: ExprId, kind: &TypedExprKind) -> Result<(), CompilerFailure> {
        match kind {
            TypedExprKind::Call { mangled, args, .. }
                if crate::stdlib::security::is_check(mangled) =>
            {
                self.values.extend(args.get(1).copied());
            }
            TypedExprKind::GenericCall { mangled, args, .. }
                if crate::stdlib::security::is_check(mangled) =>
            {
                self.values.extend(args.get(1).map(|arg| arg.expr));
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Clone, Default)]
struct Operation {
    effects: Vec<AuthorityEffect>,
    calls: Vec<Call>,
}

fn span_key(span: Span) -> (u32, u32, u32) {
    (span.file.0, span.start, span.end)
}

fn parameter_names(builder: &Builder<'_>) -> BTreeMap<usize, Vec<String>> {
    let mut by_body = BTreeMap::new();
    for function in &builder.ta.functions {
        by_body.insert(
            function.body,
            function
                .params
                .iter()
                .map(|param| param.name.name.clone())
                .collect::<Vec<_>>(),
        );
    }
    for class in builder.local_class_declarations.values() {
        for method in &class.methods {
            by_body.insert(
                method.body,
                method
                    .params
                    .iter()
                    .map(|param| param.name.name.clone())
                    .collect(),
            );
        }
    }
    builder
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            node.root
                .statements
                .first()
                .and_then(|body| by_body.get(body))
                .map(|params| (index, params.clone()))
        })
        .collect()
}
