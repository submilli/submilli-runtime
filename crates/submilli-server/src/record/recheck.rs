//! Re-check: each decision a run recorded, re-resolved under a newer blueprint.
//!
//! Nothing runs. A decision made by the policy is resolved again from its recorded
//! context; the others (a runtime invariant, a read-only volume, a quota, the egress
//! guard) do not depend on the blueprint's rules and are reported unchanged. What a
//! program would do differently once a decision changes is for a test run to find out:
//! the report marks the first decision that changed.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use interpreter::runtime::{CallRecord, DecisionRecord, EntryPath, SourceLine};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use submilli_blueprint::{
    Action, Blueprint, Resolution, ResolutionCause, VarBindings, VariableError, resolve_variables,
};

use super::{FinishedRun, McpCatalog, RunStart};

/// A recorded run, as an embedder keeps it and hands it back: what [`recheck`] reads, and
/// what a test run is built from.
#[derive(Clone, Serialize, Deserialize)]
pub struct RecordedRun {
    pub execution_id: String,
    pub blueprint_name: String,
    pub blueprint_hash: Option<String>,
    /// The blueprint version the run was decided under; see
    /// [`RunStart::blueprint_version`]. Absent from recordings made before it was kept.
    #[serde(default)]
    pub blueprint_version: Option<String>,
    /// The program's source; `None` for a file tool.
    pub code: Option<String>,
    /// The session the run executed in, whose files and data a test run copies.
    pub session_id: Option<String>,
    /// The variables the run was bound to.
    pub variables: VarBindings,
    pub decisions: Vec<DecisionRecord>,
    pub calls: Vec<CallRecord>,
    /// The recorder's caps cut the decisions or calls: some are missing.
    pub log_truncated: bool,
    #[serde(
        serialize_with = "serialize_catalog",
        deserialize_with = "deserialize_catalog"
    )]
    pub mcp_catalog: Option<Arc<McpCatalog>>,
}

fn serialize_catalog<S: Serializer>(
    catalog: &Option<Arc<McpCatalog>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    catalog.as_deref().serialize(serializer)
}

fn deserialize_catalog<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Arc<McpCatalog>>, D::Error> {
    Ok(Option::<McpCatalog>::deserialize(deserializer)?.map(Arc::new))
}

impl RecordedRun {
    /// The run `start` describes, as it ended in `finished`.
    pub fn from_parts(start: &RunStart, finished: &FinishedRun) -> Self {
        Self {
            execution_id: start.execution_id.clone(),
            blueprint_name: start.blueprint_name.clone(),
            blueprint_hash: start.blueprint_hash.clone(),
            blueprint_version: start.blueprint_version.clone(),
            code: start.code.as_deref().map(str::to_owned),
            session_id: start.session_id.clone(),
            variables: (*start.variables).clone(),
            decisions: finished.log.records.clone(),
            calls: finished.log.calls.clone(),
            log_truncated: finished.log.truncated || finished.log.calls_dropped > 0,
            mcp_catalog: finished.mcp_catalog.clone(),
        }
    }
}

/// What re-resolving one decision found.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum Verdict {
    NewlyAllowed,
    NewlyDenied,
    Unchanged,
    /// The same answer, decided by another rule (`None` is the default action).
    UnchangedDifferentRule {
        was: Option<usize>,
        now: Option<usize>,
    },
    /// The recording does not hold enough of the call to resolve it again.
    CantTell {
        reason: String,
    },
    /// A redirect hop of a request that is now denied: the program would never get there.
    NotReached {
        parent_call_index: u64,
    },
}

/// One recorded decision and how it fares under the current blueprint.
#[derive(Debug, Clone, Serialize)]
pub struct DecisionCheck {
    pub call_index: u64,
    pub caller: String,
    pub capability: String,
    pub line: Option<SourceLine>,
    pub entry_path: EntryPath,
    /// The recorded audit source. Only `policy` decisions are resolved again.
    pub source: String,
    pub rechecked: bool,
    pub was_allowed: bool,
    pub was_rule: Option<usize>,
    pub verdict: Verdict,
    /// The current blueprint's reasoning, when the decision was resolved again.
    pub now: Option<Resolution>,
    /// The first decision whose outcome changed: what follows is for a test run to find out.
    pub first_changed: bool,
}

/// Variables as a test run would bind them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct VariableReport {
    /// Recorded values the current blueprint still declares.
    pub kept: BTreeMap<String, String>,
    /// Newly declared variables, filled from the current bindings.
    pub filled: BTreeMap<String, String>,
    /// Recorded variables the current blueprint no longer declares.
    pub dropped: Vec<String>,
}

impl VariableReport {
    /// The bindings a test run would use: kept and filled values together.
    pub fn bindings(&self) -> VarBindings {
        let mut bindings = self.kept.clone();
        bindings.extend(self.filled.clone());
        bindings
    }

    /// [`bindings`](Self::bindings) with the declarations' defaults filled in and the
    /// required ones checked: what a run of `blueprint` would see. A re-check and a test
    /// run both read variables through this, so they see the same values.
    pub fn resolve(&self, blueprint: &Blueprint) -> Result<VarBindings, VariableError> {
        resolve_variables(&blueprint.variables, &self.bindings())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RecheckSummary {
    pub newly_allowed: usize,
    pub newly_denied: usize,
    pub unchanged: usize,
    pub different_rule: usize,
    pub cant_tell: usize,
    pub not_reached: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecheckReport {
    pub variables: VariableReport,
    pub decisions: Vec<DecisionCheck>,
    /// Position in `decisions` of the first changed one.
    pub first_changed: Option<usize>,
    pub summary: RecheckSummary,
    /// The recording lost decisions to its caps, so the list may be incomplete.
    pub recording_truncated: bool,
}

impl RecheckReport {
    /// Whether any decision's outcome changed.
    pub fn changed(&self) -> bool {
        self.first_changed.is_some()
    }

    /// The bindings a test run would use: kept and filled values together.
    pub fn bindings(&self) -> VarBindings {
        self.variables.bindings()
    }
}

/// Resolves `run`'s decisions again under `blueprint`, with the recorded variables kept
/// where it still declares them and `current_bindings` filling the ones it newly declares.
pub fn recheck(
    blueprint: &Blueprint,
    current_bindings: &VarBindings,
    run: &RecordedRun,
) -> RecheckReport {
    let variables = reconcile_variables(blueprint, current_bindings, &run.variables);
    // A required variable nobody bound leaves a run unable to start; the decisions are
    // still resolved, over what was bound.
    let bindings = variables
        .resolve(blueprint)
        .unwrap_or_else(|_| variables.bindings());
    // Calls with a decision that is now denied: their redirect hops are never made.
    let mut denied_calls: HashSet<u64> = HashSet::new();
    let mut summary = RecheckSummary::default();
    let mut first_changed = None;
    let mut decisions = Vec::with_capacity(run.decisions.len());
    for record in &run.decisions {
        let parent = match record.entry_path {
            EntryPath::RedirectHop {
                parent_call_index, ..
            } => Some(parent_call_index),
            _ => None,
        };
        let rechecked = record.source == "policy";
        let (verdict, now) = match parent.filter(|parent| denied_calls.contains(parent)) {
            Some(parent_call_index) => (Verdict::NotReached { parent_call_index }, None),
            None if rechecked => resolve_again(blueprint, &bindings, record),
            None => (Verdict::Unchanged, None),
        };
        match &verdict {
            Verdict::NewlyAllowed => summary.newly_allowed += 1,
            Verdict::NewlyDenied => {
                summary.newly_denied += 1;
                // A hop that is denied ends the request it belongs to.
                denied_calls.insert(parent.unwrap_or(record.call_index));
            }
            Verdict::Unchanged => summary.unchanged += 1,
            Verdict::UnchangedDifferentRule { .. } => summary.different_rule += 1,
            Verdict::CantTell { .. } => summary.cant_tell += 1,
            Verdict::NotReached { .. } => summary.not_reached += 1,
        }
        let changed = matches!(verdict, Verdict::NewlyAllowed | Verdict::NewlyDenied);
        let first = changed && first_changed.is_none();
        if first {
            first_changed = Some(decisions.len());
        }
        decisions.push(DecisionCheck {
            call_index: record.call_index,
            caller: record.caller.clone(),
            capability: record.capability.clone(),
            line: record.line,
            entry_path: record.entry_path,
            source: record.source.clone(),
            rechecked,
            was_allowed: record.allowed,
            was_rule: record.rule,
            verdict,
            now,
            first_changed: first,
        });
    }
    RecheckReport {
        variables,
        decisions,
        first_changed,
        summary,
        recording_truncated: run.log_truncated,
    }
}

pub(super) fn reconcile_variables(
    blueprint: &Blueprint,
    current: &VarBindings,
    recorded: &VarBindings,
) -> VariableReport {
    let mut report = VariableReport::default();
    for (name, value) in recorded {
        if blueprint.variables.contains_key(name) {
            report.kept.insert(name.clone(), value.clone());
        } else {
            report.dropped.push(name.clone());
        }
    }
    for name in blueprint.variables.keys() {
        if !recorded.contains_key(name)
            && let Some(value) = current.get(name)
        {
            report.filled.insert(name.clone(), value.clone());
        }
    }
    report
}

fn resolve_again(
    blueprint: &Blueprint,
    bindings: &VarBindings,
    record: &DecisionRecord,
) -> (Verdict, Option<Resolution>) {
    // A cut context could resolve either way, so the answer would be a guess.
    let gap = if record.payload_dropped {
        Some("the context was dropped to fit the recorder's budget")
    } else if record.context_truncated {
        Some("the recorded context was cut, so a filter may see other values")
    } else {
        None
    };
    if let Some(reason) = gap {
        return (
            Verdict::CantTell {
                reason: reason.into(),
            },
            None,
        );
    }
    let now = blueprint.explain_permission(
        &record.caller,
        &record.capability,
        &record.context,
        bindings,
    );
    let now_allowed = now.action == Action::Allow;
    let now_rule = match &now.cause {
        ResolutionCause::Rule(rule) => Some(rule.index),
        ResolutionCause::Default { .. } => None,
    };
    let verdict = match (record.allowed, now_allowed) {
        (false, true) => Verdict::NewlyAllowed,
        (true, false) => Verdict::NewlyDenied,
        _ if record.rule != now_rule => Verdict::UnchangedDifferentRule {
            was: record.rule,
            now: now_rule,
        },
        _ => Verdict::Unchanged,
    };
    (verdict, Some(now))
}
