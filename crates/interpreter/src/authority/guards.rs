//! Must-analysis of successful public-route checks, separate from selector and
//! egress proofs. Helpers carry incoming authority but cannot create it.

mod control_flow;
#[cfg(test)]
mod tests;

use super::*;
use control_flow::{Action, FlowGraph, FlowNode, ThrowKind};

#[derive(Default)]
pub(super) struct Analysis {
    routes: BTreeMap<usize, Vec<AuthorityRouteEffect>>,
    warnings: BTreeMap<usize, Vec<Diagnostic>>,
}

impl Analysis {
    pub(super) fn has_lookup(&self, route: usize) -> bool {
        self.routes.get(&route).is_some_and(|effects| {
            effects
                .iter()
                .any(|effect| effect.guard.status == AuthorityGuardStatus::Lookup)
        })
    }

    pub(super) fn apply(
        &self,
        route: usize,
        effects: &mut Vec<AuthorityRouteEffect>,
        warnings: &mut Vec<Diagnostic>,
    ) {
        if let Some(analysed) = self.routes.get(&route) {
            // Retain full call-graph coverage, including unreachable or latent
            // effects. Only the executed control-flow analysis proves ordering.
            let observed: BTreeSet<_> = analysed.iter().map(|effect| &effect.effect).collect();
            effects.retain_mut(|effect| {
                effect.guard.status = AuthorityGuardStatus::Unreachable;
                !observed.contains(&effect.effect)
            });
            effects.extend(analysed.iter().cloned());
        }
        if let Some(found) = self.warnings.get(&route) {
            warnings.extend(found.iter().cloned());
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct CheckFacts {
    names: BTreeSet<String>,
    sites: BTreeSet<AuthoritySpan>,
    dependencies: BTreeSet<String>,
    dynamic: bool,
    scope_invalidated: bool,
}

impl CheckFacts {
    fn merge(&mut self, other: &Self) {
        self.names.retain(|name| other.names.contains(name));
        self.sites.extend(other.sites.iter().cloned());
        self.dependencies.extend(other.dependencies.iter().cloned());
        self.dynamic |= other.dynamic;
        self.scope_invalidated |= other.scope_invalidated;
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct State {
    /// An empty key means some check succeeded on every incoming path. Other
    /// keys describe minimal item/collection identity, not selector provenance.
    scopes: BTreeMap<String, CheckFacts>,
    path: Vec<AuthorityControlStep>,
}

impl State {
    fn merge(&mut self, other: &Self) -> bool {
        let before = self.scopes.clone();
        self.scopes.retain(|scope, facts| {
            let Some(incoming) = other.scopes.get(scope) else {
                return false;
            };
            facts.merge(incoming);
            true
        });
        if (before.contains_key("") && !other.scopes.contains_key(""))
            || (!self.scopes.contains_key("") && other.path.len() < self.path.len())
        {
            self.path = other.path.clone();
        }
        before != self.scopes
    }

    fn record_step(&mut self, step: AuthorityControlStep) {
        if self.path.len() >= 64 {
            // Nonempty by the length check; this local vector is never exposed.
            self.path.remove(0);
            if let Some(first) = self.path.first_mut() {
                first.description = "earlier witness steps omitted".into();
            }
        }
        self.path.push(step);
    }

    fn size(&self) -> usize {
        self.scopes.values().fold(self.path.len(), |total, facts| {
            total
                .saturating_add(facts.names.len())
                .saturating_add(facts.sites.len())
                .saturating_add(facts.dependencies.len())
        })
    }

    fn invalidate(&mut self, binding: Option<&str>) {
        self.scopes.retain(|scope, facts| {
            let valid = facts.dependencies.is_empty()
                || binding.is_some_and(|binding| {
                    !facts
                        .dependencies
                        .iter()
                        .any(|dependency| reference_matches(dependency, binding))
                });
            if scope.is_empty() {
                // Mutation cannot undo successful-check ordering, but known
                // sinks must not reuse its invalidated applicability evidence.
                facts.scope_invalidated |= !valid;
                true
            } else {
                valid
            }
        });
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Summary {
    normal: bool,
    throws: BTreeSet<ThrowKind>,
    effects: BTreeMap<AuthorityEffect, Vec<AuthorityWitnessStep>>,
    mutates: bool,
    loop_scopes: BTreeMap<AuthorityEffect, BTreeSet<String>>,
    unproven: BTreeSet<AuthorityEffect>,
    invalidated_effects: BTreeSet<AuthorityEffect>,
}

pub(super) fn analyse(builder: &Builder<'_>) -> Result<Analysis, CompilerFailure> {
    let mut work = 0;
    let graphs = control_flow::build(builder, &mut work)?;
    let summaries = summarize(builder, &graphs, &mut work)?;
    let mut result = Analysis::default();
    let mut total_observations = 0;
    for (route, node) in builder.nodes.iter().enumerate() {
        if node.semantic_routes.is_empty() {
            continue;
        }
        let effects = analyse_route(builder, &graphs[route], &summaries, &mut work)?;
        total_observations = checked_authority_budget(
            total_observations,
            effects.len(),
            builder.limits.route_effects,
            node.raw_span,
            "guard observation",
        )?;
        let has_lookup = effects
            .iter()
            .any(|effect| effect.guard.status == AuthorityGuardStatus::Lookup);
        let mut warnings = Vec::new();
        for effect in &effects {
            let status = effect.guard.status;
            if status == AuthorityGuardStatus::Checked {
                continue;
            }
            if status == AuthorityGuardStatus::Unguarded
                && !node.has_direct_semantic_check
                && !has_lookup
            {
                // SUB-1421 already supplies the missing-direct-check diagnostic.
                continue;
            }
            for (name, span) in &node.semantic_routes {
                warnings.push(guard_warning(builder, *span, name, effect)?);
            }
        }
        result.routes.insert(route, effects);
        result.warnings.insert(route, warnings);
    }
    Ok(result)
}

fn summarize(
    builder: &Builder<'_>,
    graphs: &[FlowGraph],
    work: &mut usize,
) -> Result<Vec<Summary>, CompilerFailure> {
    let mut summaries = vec![Summary::default(); graphs.len()];
    // Least fixed point: a recursive call has no normal/exceptional successor
    // until a finite path establishes one. This avoids inventing authority or
    // returning from unconditional recursion.
    loop {
        let mut changed = false;
        for (caller, graph) in graphs.iter().enumerate() {
            let mut summary = Summary::default();
            let mut seen = BTreeSet::new();
            let mut queue = VecDeque::from([(graph.entry, false)]);
            while let Some((index, mutated)) = queue.pop_front() {
                if !seen.insert((index, mutated)) {
                    continue;
                }
                budget(builder, work, graph.nodes[index].span, 1)?;
                let node = &graph.nodes[index];
                match &node.action {
                    Action::Exit => summary.normal = true,
                    Action::ThrowExit(kind) => {
                        summary.throws.insert(*kind);
                    }
                    Action::Invalidate(None) | Action::Coverage => summary.mutates = true,
                    _ => {}
                }
                summary.mutates |= node.effects.iter().any(|effect| effect.unresolved);
                for effect in &node.effects {
                    summary.effects.insert(effect.clone(), Vec::new());
                    if mutated {
                        summary.invalidated_effects.insert(effect.clone());
                    }
                    if matches!(node.action, Action::Coverage) {
                        summary.unproven.insert(effect.clone());
                    }
                    for scopes in &node.item_scopes {
                        let needed = summary.loop_scopes.entry(effect.clone()).or_default();
                        needed.extend(scopes.iter().skip(1).cloned());
                        if scopes.len() < 2 {
                            needed.insert("?loop".into());
                        }
                    }
                }
                for edge in &node.calls {
                    let target = &summaries[edge.target];
                    summary.mutates |= target.mutates;
                    summary.unproven.extend(target.unproven.iter().cloned());
                    for (effect, witness) in &target.effects {
                        if matches!(node.action, Action::Coverage) {
                            summary.unproven.insert(effect.clone());
                        }
                        if mutated || target.invalidated_effects.contains(effect) {
                            summary.invalidated_effects.insert(effect.clone());
                        }
                        budget(builder, work, node.span, witness.len().saturating_add(1))?;
                        let mut steps = vec![edge.witness.clone()];
                        steps.extend(witness.iter().cloned());
                        for scopes in &node.item_scopes {
                            let needed = summary.loop_scopes.entry(effect.clone()).or_default();
                            needed.extend(scopes.iter().skip(1).cloned());
                            if scopes.len() < 2 {
                                needed.insert("?loop".into());
                            }
                        }
                        let existing = summary
                            .effects
                            .entry(effect.clone())
                            .or_insert_with(|| steps.clone());
                        if let Some(required) = target.loop_scopes.get(effect) {
                            summary
                                .loop_scopes
                                .entry(effect.clone())
                                .or_default()
                                .extend(substitute_scopes(required, edge));
                        }
                        if steps.len() < existing.len() {
                            *existing = steps;
                        }
                    }
                }
                let mutated_after = mutated
                    || matches!(node.action, Action::Invalidate(None) | Action::Coverage)
                    || node.effects.iter().any(|effect| effect.unresolved)
                    || node.calls.iter().any(|call| summaries[call.target].mutates);
                queue.extend(
                    successors(node, &summaries)
                        .into_iter()
                        .map(|(index, _)| (index, mutated_after)),
                );
            }
            if summary != summaries[caller] {
                summaries[caller] = summary;
                changed = true;
            }
        }
        if !changed {
            return Ok(summaries);
        }
    }
}

fn successors(node: &FlowNode, summaries: &[Summary]) -> Vec<(usize, bool)> {
    let mut next = Vec::new();
    let returns = node.calls.iter().all(|call| summaries[call.target].normal);
    if returns {
        next.extend(node.normal.iter().map(|index| (*index, false)));
    }
    let mut thrown = node.throws.clone();
    for call in &node.calls {
        thrown.extend(summaries[call.target].throws.iter().copied());
    }
    for kind in thrown {
        next.extend(node.exceptional[&kind].iter().map(|index| (*index, true)));
    }
    next
}

fn analyse_route(
    builder: &Builder<'_>,
    graph: &FlowGraph,
    summaries: &[Summary],
    work: &mut usize,
) -> Result<Vec<AuthorityRouteEffect>, CompilerFailure> {
    let mut inputs: Vec<Option<State>> = vec![None; graph.nodes.len()];
    inputs[graph.entry] = Some(State::default());
    let mut queue = VecDeque::from([graph.entry]);
    let mut observations = BTreeMap::new();
    while let Some(index) = queue.pop_front() {
        let node = &graph.nodes[index];
        budget(builder, work, node.span, 1)?;
        // Only initialized entries are enqueued; the vector is local and its
        // graph-indexed slots cannot be resized or changed by callbacks.
        let input = inputs[index]
            .as_ref()
            .expect("queued flow node has an input state");
        budget(builder, work, node.span, input.size())?;
        let mut state = input.clone();
        if let Some(description) = &node.description {
            state.record_step(AuthorityControlStep {
                span: source_span(builder.sources, node.span)?,
                description: description.clone(),
            });
        }
        for effect in &node.effects {
            observe(
                builder,
                node,
                &state,
                effect,
                EffectContext {
                    witness: &[],
                    required: &BTreeSet::new(),
                    unproven: matches!(node.action, Action::Coverage),
                    index,
                },
                &mut observations,
            )?;
        }
        for call in &node.calls {
            for (effect, witness) in &summaries[call.target].effects {
                budget(builder, work, node.span, witness.len().saturating_add(1))?;
                let mut steps = vec![call.witness.clone()];
                steps.extend(witness.iter().cloned());
                let required = summaries[call.target]
                    .loop_scopes
                    .get(effect)
                    .map(|required| substitute_scopes(required, call))
                    .unwrap_or_default();
                let mut effect_state = state.clone();
                if summaries[call.target].invalidated_effects.contains(effect) {
                    effect_state.invalidate(None);
                }
                observe(
                    builder,
                    node,
                    &effect_state,
                    effect,
                    EffectContext {
                        witness: &steps,
                        required: &required,
                        unproven: matches!(node.action, Action::Coverage)
                            || summaries[call.target].unproven.contains(effect),
                        index,
                    },
                    &mut observations,
                )?;
            }
        }
        let mut normal = state.clone();
        match &node.action {
            Action::Check { name, roots } => {
                let facts = CheckFacts {
                    names: name.iter().cloned().collect(),
                    sites: BTreeSet::from([source_span(builder.sources, node.span)?]),
                    dependencies: roots.clone(),
                    dynamic: name.is_none()
                        || roots.iter().any(|root| root.starts_with("?shadowed")),
                    scope_invalidated: false,
                };
                // Computed payloads have no proven item/collection identity.
                // Record normal-success ordering, but leave selector scope to
                // SUB-1423 rather than treating arguments as returned values.
                let scoped_roots = roots
                    .iter()
                    .filter(|_| !roots.iter().any(|root| root.starts_with("?computed")));
                for scope in std::iter::once(String::new())
                    .chain(scoped_roots.cloned())
                    .chain(roots.is_empty().then_some("*".into()))
                {
                    // A later successful check can replace weaker evidence for
                    // this scope; previous names still succeeded on this path.
                    if let Some(previous) = normal.scopes.get_mut(&scope) {
                        previous.names.extend(facts.names.iter().cloned());
                        previous.sites.extend(facts.sites.iter().cloned());
                        previous
                            .dependencies
                            .extend(facts.dependencies.iter().cloned());
                        previous.dynamic &= facts.dynamic;
                        previous.scope_invalidated = false;
                    } else {
                        normal.scopes.insert(scope, facts.clone());
                    }
                }
            }
            Action::Invalidate(binding) => normal.invalidate(binding.as_deref()),
            Action::Coverage => {
                normal.invalidate(None);
                state.invalidate(None);
            }
            Action::Iteration => normal.scopes.retain(|scope, facts| {
                let indexed = facts.dependencies.iter().any(|root| root.ends_with("[]"));
                if scope.is_empty() {
                    facts.scope_invalidated |= indexed;
                    true
                } else {
                    !indexed
                }
            }),
            _ => {}
        }
        let mutates = node.effects.iter().any(|effect| effect.unresolved)
            || node.calls.iter().any(|call| summaries[call.target].mutates);
        if mutates {
            normal.invalidate(None);
            state.invalidate(None);
        }
        for (successor, exceptional) in successors(node, summaries) {
            let mut outgoing = if exceptional {
                state.clone()
            } else {
                normal.clone()
            };
            if exceptional && matches!(node.action, Action::Check { .. }) {
                outgoing.record_step(AuthorityControlStep {
                    span: source_span(builder.sources, node.span)?,
                    description: "check denied; no authority was added".into(),
                });
            }
            match &mut inputs[successor] {
                Some(input) => {
                    if input.merge(&outgoing) {
                        queue.push_back(successor);
                    }
                }
                input @ None => {
                    *input = Some(outgoing);
                    queue.push_back(successor);
                }
            }
        }
    }
    let mut effects: Vec<_> = observations.into_values().collect();
    effects.sort_by(|left, right| left.effect.cmp(&right.effect));
    Ok(effects)
}

type Observations = BTreeMap<(usize, AuthorityEffect), AuthorityRouteEffect>;

struct EffectContext<'a> {
    witness: &'a [AuthorityWitnessStep],
    required: &'a BTreeSet<String>,
    unproven: bool,
    index: usize,
}

fn observe(
    builder: &Builder<'_>,
    node: &FlowNode,
    state: &State,
    effect: &AuthorityEffect,
    context: EffectContext<'_>,
    observations: &mut Observations,
) -> Result<(), CompilerFailure> {
    let EffectContext {
        witness,
        required,
        unproven,
        index,
    } = context;
    // Unknown dispatch still occurs at this modeled invocation point. Proving
    // an earlier check does not require discovering its target. Item selectors
    // apply to known sinks; unknown effects retain unresolved discovery metadata.
    let known_sink = effect.capability.is_some() && !effect.unresolved;
    let facts = if node.item_scopes.is_empty() || !known_sink {
        state
            .scopes
            .get("")
            .filter(|facts| !known_sink || !facts.scope_invalidated)
            .or_else(|| state.scopes.get("*"))
    } else {
        // Each iterated value needs a matching item or whole-collection scope.
        // Checks with no payload roots explicitly describe route-wide work.
        state.scopes.get("*").or_else(|| {
            node.item_scopes
                .iter()
                .find_map(|scopes| scopes.iter().find_map(|scope| state.scopes.get(scope)))
                .filter(|_| {
                    node.item_scopes
                        .iter()
                        .all(|scopes| scopes.iter().any(|scope| state.scopes.contains_key(scope)))
                })
        })
    };
    let facts = facts.filter(|facts| {
        !known_sink
            || (!facts.scope_invalidated
                && (facts.dependencies.is_empty()
                    || required
                        .iter()
                        .all(|scope| state.scopes.contains_key(scope))))
    });
    let status = if unproven {
        AuthorityGuardStatus::Unproven
    } else if let Some(facts) = facts {
        if facts.dynamic {
            AuthorityGuardStatus::Unproven
        } else {
            AuthorityGuardStatus::Checked
        }
    } else if state.scopes.contains_key("")
        || !known_sink
        || required.iter().any(|scope| scope.starts_with('?'))
        || (!node.item_scopes.is_empty()
            && state
                .scopes
                .get("")
                .is_some_and(|facts| facts.dependencies.contains("?computed")))
    {
        AuthorityGuardStatus::Unproven
    } else if node.lookup && is_lookup_effect(effect) {
        AuthorityGuardStatus::Lookup
    } else {
        AuthorityGuardStatus::Unguarded
    };
    let mut path = state.path.clone();
    path.push(AuthorityControlStep {
        span: source_span(builder.sources, node.span)?,
        description: "effect is invoked on this path".into(),
    });
    let evidence = facts.or_else(|| state.scopes.get(""));
    let guard = AuthorityGuardEvidence {
        status,
        capabilities: evidence.map_or_else(Vec::new, |facts| facts.names.iter().cloned().collect()),
        checks: evidence.map_or_else(Vec::new, |facts| facts.sites.iter().cloned().collect()),
        path,
    };
    let key = (index, effect.clone());
    if let Some(previous) = observations.get_mut(&key) {
        // Dataflow can only lose must facts. Keep the final, weaker observation.
        previous.guard = guard;
    } else {
        check_limit(
            observations.len(),
            builder.limits.route_effects,
            node.span,
            "guard observation",
        )?;
        observations.insert(
            key,
            AuthorityRouteEffect {
                effect: effect.clone(),
                witness: witness.to_vec(),
                guard,
            },
        );
    }
    Ok(())
}

fn is_lookup_effect(effect: &AuthorityEffect) -> bool {
    matches!(
        effect.capability.as_deref(),
        Some(
            "http.get"
                | "fs.read"
                | "fs.stat"
                | "fs.list"
                | "secrets.get"
                | "session.read"
                | "git.read"
        )
    )
}

fn guard_warning(
    builder: &Builder<'_>,
    span: Span,
    route: &str,
    effect: &AuthorityRouteEffect,
) -> Result<Diagnostic, CompilerFailure> {
    let capability = effect
        .effect
        .capability
        .as_deref()
        .unwrap_or("unresolved authority");
    let detail = match effect.guard.status {
        AuthorityGuardStatus::Lookup => {
            "performs a pre-check authorization lookup; denial-path confinement is unproven"
        }
        AuthorityGuardStatus::Unproven => {
            "has an authority coverage gap; authorization cannot be proved"
        }
        _ => "reaches an effect without a successful direct semantic check on every path",
    };
    let mut notes = Vec::new();
    for step in &effect.guard.path {
        notes.push((
            raw_span(builder.sources, &step.span)?,
            step.description.clone(),
        ));
    }
    for step in &effect.witness {
        notes.push((
            raw_span(builder.sources, &step.span)?,
            format!("`{}` is called here", step.target),
        ));
    }
    notes.push((
        raw_span(builder.sources, &effect.effect.sink)?,
        format!("the `{capability}` operation is reached here"),
    ));
    Ok(Diagnostic {
        severity: Severity::Warning, span,
        message: format!("public route `{route}` {detail} (`{capability}`)"),
        help: vec![match effect.guard.status {
            AuthorityGuardStatus::Lookup => "Review lookup confinement separately; SUB-1426 will prove that results cannot escape on denial".into(),
            AuthorityGuardStatus::Unproven => "Use statically resolvable checks and calls, or review the unsupported path explicitly".into(),
            _ => "Move the check before the effect on every continuing path; return or rethrow when authorization is denied".into(),
        }], notes,
    })
}

// AuthoritySpan intentionally contains portable paths rather than FileId. The
// diagnostic needs the original span, recovered by the analysis's source table.
fn raw_span(sources: &Sources, span: &AuthoritySpan) -> Result<Span, CompilerFailure> {
    let (file, _) = sources.find_path(&span.path).ok_or_else(|| {
        crate::typechecker::invariant_failure("guard evidence references an unknown source file")
    })?;
    Ok(Span {
        file,
        start: span.start.byte,
        end: span.end.byte,
    })
}

fn reference_matches(reference: &str, binding: &str) -> bool {
    reference == binding
        || reference
            .strip_prefix(binding)
            .is_some_and(|suffix| suffix.starts_with('[') || suffix.starts_with('.'))
}

fn budget(
    builder: &Builder<'_>,
    work: &mut usize,
    span: Span,
    additional: usize,
) -> Result<(), CompilerFailure> {
    *work = checked_authority_budget(
        *work,
        additional,
        builder.limits.witness_work,
        span,
        "guard work",
    )?;
    Ok(())
}

fn substitute_scopes(required: &BTreeSet<String>, call: &control_flow::Call) -> BTreeSet<String> {
    required
        .iter()
        .flat_map(|scope| {
            call.bindings
                .get(scope)
                .cloned()
                .unwrap_or_else(|| BTreeSet::from(["?loop".into()]))
        })
        .collect()
}
