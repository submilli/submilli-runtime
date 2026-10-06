//! Re-check: recorded decisions resolved again under a newer blueprint, without running
//! anything.

use std::sync::Arc;
use std::time::Duration;

use interpreter::runtime::limits::ExecutionUsage;
use interpreter::runtime::{
    DecisionAction, DecisionCause, DecisionLogOutput, DecisionRecord, EntryPath,
};
use serde_json::{Value, json};
use submilli_blueprint::{Blueprint, ResolutionCause, VarBindings};
use submilli_server::record::recheck::Verdict;
use submilli_server::record::{FinishedRun, McpCatalog, RecordedRun, RunEntry, RunStart, recheck};

fn blueprint(yaml: &str) -> Blueprint {
    submilli_blueprint::parse(yaml).expect("blueprint")
}

fn http_context(host: &str) -> Value {
    json!({ "host": host, "path": "/", "body_size": 0, "timeout_ms": 30000 })
}

/// A decision as the recorder keeps it, resolved by the policy.
fn decision(
    call_index: u64,
    capability: &str,
    context: Value,
    rule: Option<usize>,
) -> DecisionRecord {
    DecisionRecord {
        call_index,
        seq: 1,
        at_micros: 0,
        caller: "main".into(),
        capability: capability.into(),
        context,
        context_truncated: false,
        context_digest: 0,
        allowed: false,
        action: DecisionAction::Deny,
        cause: DecisionCause::Default { caller_block: true },
        near_misses: Vec::new(),
        source: "policy".into(),
        rule,
        reason: None,
        entry_path: EntryPath::GatedOp,
        line: None,
        filtered: false,
        payload_dropped: false,
    }
}

fn allowed(mut record: DecisionRecord) -> DecisionRecord {
    record.allowed = true;
    record.action = DecisionAction::Allow;
    record
}

fn hop(mut record: DecisionRecord, parent: u64, index: u32) -> DecisionRecord {
    record.entry_path = EntryPath::RedirectHop {
        parent_call_index: parent,
        index,
    };
    record
}

fn recorded(variables: &[(&str, &str)], decisions: Vec<DecisionRecord>) -> RecordedRun {
    RecordedRun {
        execution_id: "run-1".into(),
        blueprint_name: "bp".into(),
        blueprint_hash: None,
        code: Some("function main(): string { return \"x\"; }".into()),
        session_id: None,
        variables: variables
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect(),
        decisions,
        calls: Vec::new(),
        log_truncated: false,
        mcp_catalog: None,
    }
}

fn verdicts(report: &submilli_server::record::RecheckReport) -> Vec<Verdict> {
    report.decisions.iter().map(|d| d.verdict.clone()).collect()
}

const DENY_ALL: &str = "name: bp\ndefault: deny\n";

fn allow_get_for(host: &str) -> String {
    format!(
        "name: bp\ndefault: deny\npermissions:\n  main:\n    - name: reads\n      capability: http.get\n      filter: host == \"{host}\"\n      action: allow\n"
    )
}

#[test]
fn a_rule_added_for_a_denied_call_shows_it_newly_allowed_under_the_new_rule() {
    let run = recorded(
        &[],
        vec![decision(0, "http.get", http_context("api.test"), None)],
    );
    let report = recheck(
        &blueprint(&allow_get_for("api.test")),
        &VarBindings::new(),
        &run,
    );
    assert_eq!(verdicts(&report), [Verdict::NewlyAllowed]);
    let now = report.decisions[0].now.as_ref().expect("the new reasoning");
    assert!(matches!(
        &now.cause,
        ResolutionCause::Rule(rule) if rule.index == 0 && rule.name.as_deref() == Some("reads")
    ));
    assert_eq!(report.first_changed, Some(0));
    assert!(report.decisions[0].first_changed);
    assert_eq!(report.summary.newly_allowed, 1);
}

#[test]
fn a_narrowed_filter_shows_the_call_newly_denied_with_the_failing_comparison() {
    let run = recorded(
        &[],
        vec![allowed(decision(
            0,
            "http.get",
            http_context("api.test"),
            Some(0),
        ))],
    );
    let report = recheck(
        &blueprint(&allow_get_for("other.test")),
        &VarBindings::new(),
        &run,
    );
    assert_eq!(verdicts(&report), [Verdict::NewlyDenied]);
    let now = report.decisions[0].now.as_ref().expect("the new reasoning");
    assert_eq!(now.near_misses.len(), 1);
    let failure = &now.near_misses[0].failures[0];
    assert_eq!(failure.comparison, "host == \"other.test\"");
    assert_eq!(failure.actual, Some(json!("api.test")));
}

#[test]
fn swapping_two_rules_shows_unchanged_but_decided_by_a_different_rule() {
    let run = recorded(
        &[],
        vec![allowed(decision(
            0,
            "http.get",
            http_context("api.test"),
            Some(0),
        ))],
    );
    let swapped = blueprint(
        "name: bp\ndefault: deny\npermissions:\n  main:\n    - capability: http.get\n      filter: host == \"a.test\"\n      action: allow\n    - capability: http.get\n      filter: host == \"api.test\"\n      action: allow\n",
    );
    let report = recheck(&swapped, &VarBindings::new(), &run);
    assert_eq!(
        verdicts(&report),
        [Verdict::UnchangedDifferentRule {
            was: Some(0),
            now: Some(1)
        }]
    );
    assert_eq!(report.first_changed, None, "the outcome is the same");
}

#[test]
fn a_truncated_or_dropped_context_cannot_be_told() {
    let mut cut = decision(0, "http.get", http_context("api.test"), None);
    cut.context_truncated = true;
    let mut dropped = decision(1, "http.get", Value::Null, None);
    dropped.payload_dropped = true;
    let run = recorded(&[], vec![cut, dropped]);
    let report = recheck(
        &blueprint(&allow_get_for("api.test")),
        &VarBindings::new(),
        &run,
    );
    let reasons: Vec<_> = verdicts(&report)
        .into_iter()
        .map(|verdict| match verdict {
            Verdict::CantTell { reason } => reason,
            other => panic!("expected can't tell, got {other:?}"),
        })
        .collect();
    assert!(reasons[0].contains("cut"), "{reasons:?}");
    assert!(reasons[1].contains("dropped"), "{reasons:?}");
    assert_eq!(report.summary.cant_tell, 2);
    assert!(!report.changed());
}

#[test]
fn a_narrowed_redirect_target_shows_the_hop_newly_denied_and_later_hops_not_reached() {
    let run = recorded(
        &[],
        vec![
            allowed(decision(0, "http.get", http_context("api.test"), Some(0))),
            allowed(hop(
                decision(0, "http.get", http_context("cdn.test"), Some(1)),
                0,
                0,
            )),
            allowed(hop(
                decision(0, "http.get", http_context("cdn.test"), Some(1)),
                0,
                1,
            )),
        ],
    );
    let both = blueprint(
        "name: bp\ndefault: deny\npermissions:\n  main:\n    - capability: http.get\n      filter: host == \"api.test\"\n      action: allow\n    - capability: http.get\n      filter: host == \"cdn.test\"\n      action: allow\n",
    );
    assert!(!recheck(&both, &VarBindings::new(), &run).changed());

    let report = recheck(
        &blueprint(&allow_get_for("api.test")),
        &VarBindings::new(),
        &run,
    );
    assert_eq!(
        verdicts(&report),
        [
            Verdict::Unchanged,
            Verdict::NewlyDenied,
            Verdict::NotReached {
                parent_call_index: 0
            },
        ]
    );
    assert_eq!(report.first_changed, Some(1));
}

#[test]
fn a_hop_under_a_call_that_is_now_denied_is_not_reached() {
    let run = recorded(
        &[],
        vec![
            allowed(decision(3, "http.get", http_context("api.test"), Some(0))),
            allowed(hop(
                decision(3, "http.get", http_context("api.test"), Some(0)),
                3,
                0,
            )),
        ],
    );
    let report = recheck(&blueprint(DENY_ALL), &VarBindings::new(), &run);
    assert_eq!(
        verdicts(&report),
        [
            Verdict::NewlyDenied,
            Verdict::NotReached {
                parent_call_index: 3
            }
        ]
    );
    assert!(report.decisions[1].now.is_none());
    assert_eq!(report.first_changed, Some(0));
}

#[test]
fn other_sources_are_reported_unchanged_and_labelled() {
    let sources = ["invariant", "read_only", "quota", "egress_guard"];
    let decisions = sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            let mut record = decision(index as u64, "fs.write", json!({ "path": "/a" }), None);
            record.source = (*source).into();
            record
        })
        .collect();
    // A blueprint that would allow all of them changes nothing for these.
    let report = recheck(
        &blueprint("name: bp\ndefault: allow\n"),
        &VarBindings::new(),
        &recorded(&[], decisions),
    );
    assert_eq!(report.summary.unchanged, 4);
    assert!(!report.changed());
    for (check, source) in report.decisions.iter().zip(sources) {
        assert_eq!(check.source, source);
        assert!(!check.rechecked);
        assert!(check.now.is_none());
    }
}

#[test]
fn newly_declared_variables_are_filled_and_removed_ones_dropped() {
    let run = recorded(
        &[("customerId", "cus_northwind"), ("legacy", "x")],
        vec![decision(0, "http.get", http_context("api.test"), None)],
    );
    let current = blueprint(
        "name: bp\ndefault: deny\nvariables:\n  customerId: {}\n  region: {}\npermissions:\n  main:\n    - capability: http.get\n      filter: host == ${vars.region}\n      action: allow\n",
    );
    let bindings = VarBindings::from([
        ("customerId".to_owned(), "cus_other".to_owned()),
        ("region".to_owned(), "api.test".to_owned()),
    ]);
    let report = recheck(&current, &bindings, &run);
    // The recorded value wins where the blueprint still declares the variable.
    assert_eq!(
        report.variables.kept,
        VarBindings::from([("customerId".to_owned(), "cus_northwind".to_owned())])
    );
    assert_eq!(
        report.variables.filled,
        VarBindings::from([("region".to_owned(), "api.test".to_owned())])
    );
    assert_eq!(report.variables.dropped, ["legacy"]);
    assert_eq!(report.bindings().len(), 2);
    // The filled value is what the filter saw.
    assert_eq!(verdicts(&report), [Verdict::NewlyAllowed]);
}

#[test]
fn a_declared_default_is_what_a_filter_sees_when_nothing_is_bound() {
    let run = recorded(
        &[],
        vec![decision(0, "http.get", http_context("api.test"), None)],
    );
    let current = blueprint(
        "name: bp\ndefault: deny\nvariables:\n  region: {default: api.test}\npermissions:\n  main:\n    - capability: http.get\n      filter: host == ${vars.region}\n      action: allow\n",
    );
    let report = recheck(&current, &VarBindings::new(), &run);
    // A test run resolves the same declarations, so it would see the same host.
    assert!(report.variables.filled.is_empty());
    assert_eq!(verdicts(&report), [Verdict::NewlyAllowed]);
}

#[test]
fn an_unchanged_blueprint_yields_no_changes() {
    let yaml = allow_get_for("api.test");
    let run = recorded(
        &[],
        vec![
            allowed(decision(0, "http.get", http_context("api.test"), Some(0))),
            decision(1, "http.post", http_context("api.test"), None),
        ],
    );
    let report = recheck(&blueprint(&yaml), &VarBindings::new(), &run);
    assert_eq!(verdicts(&report), [Verdict::Unchanged, Verdict::Unchanged]);
    assert!(!report.changed());
    assert_eq!(report.first_changed, None);
    assert!(report.decisions.iter().all(|check| !check.first_changed));
}

#[test]
fn a_recorded_run_is_built_from_its_start_and_end_and_serializes() {
    let start = RunStart {
        execution_id: "run-9".into(),
        label: "tester".into(),
        entry: RunEntry::Program,
        test_of: None,
        client: None,
        tool_call_id: None,
        session_id: None,
        idempotency_key: None,
        blueprint_name: "bp".into(),
        blueprint: Arc::new(blueprint(DENY_ALL)),
        blueprint_hash: Some("abc".into()),
        variables: Arc::new(VarBindings::from([("a".to_owned(), "b".to_owned())])),
        harness_secrets: Arc::default(),
        code: Some(Arc::from("function main(): void {}")),
    };
    let finished = FinishedRun {
        dispatched: true,
        error: None,
        result: None,
        console: String::new(),
        usage: ExecutionUsage::default(),
        log: DecisionLogOutput {
            records: vec![decision(0, "http.get", http_context("api.test"), None)],
            truncated: true,
            ..DecisionLogOutput::default()
        },
        mcp_catalog: Some(Arc::new(McpCatalog::empty())),
        wall: Duration::ZERO,
    };
    let run = RecordedRun::from_parts(&start, &finished);
    assert_eq!(run.execution_id, "run-9");
    assert_eq!(run.code.as_deref(), Some("function main(): void {}"));
    assert_eq!(run.variables.get("a").map(String::as_str), Some("b"));
    assert!(run.log_truncated);
    assert_eq!(run.decisions.len(), 1);
    let value = serde_json::to_value(&run).expect("serializes");
    assert_eq!(value["execution_id"], "run-9");
    assert!(value["mcp_catalog"].is_object(), "{value}");
    let report = recheck(&blueprint(DENY_ALL), &VarBindings::new(), &run);
    assert!(report.recording_truncated);
    serde_json::to_value(&report).expect("the report serializes");
}
