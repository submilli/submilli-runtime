//! Stores built for the controls' tests: runs written with the decisions a test picks,
//! and blueprint versions in the change log.

use std::collections::BTreeMap;

use interpreter::runtime::limits::ExecutionUsage;
use interpreter::runtime::{
    CallOutcome, CallRecord, DecisionAction, DecisionCause, DecisionRecord, EntryPath,
    FailureReasonRecord, FailureRecord, NearMissRecord, RuleCitation,
};
use serde_json::{Value, json};
use submilli_server::record::RecordedRun;

use crate::commands::playground::packages::ClosureEntry;
use crate::commands::playground::store::Store;
use crate::commands::playground::store::changes::{NewVersion, version_tag};
use crate::commands::playground::store::run::{RunLink, StoredError, StoredRun, StoredTestReport};

use super::Reader;
use super::render::Page;

/// The starter blueprint, as the change log would hold it.
pub(crate) const BLUEPRINT: &str = "kind: blueprint
name: billing
variables:
  customerId:
    required: true
default: deny
permissions:
  main:
  - name: charges-for-signed-in-customer
    capability: acme.com/charges.list
    filter: customerId == ${vars.customerId}
    action: allow
";

pub(crate) struct Fixture {
    _dir: tempfile::TempDir,
    pub(crate) store: Store,
    clock: u64,
}

impl Fixture {
    pub(crate) fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("store")).unwrap();
        Self {
            _dir: dir,
            store,
            // Before any line the session log writes now.
            clock: 1_700_000_000_000_000,
        }
    }

    /// Logs `text` as the next blueprint version.
    pub(crate) fn version(&self, text: &str, classification: Value, summary: &str) -> u64 {
        self.store
            .append_version(NewVersion {
                hash: format!("h{}", text.len()),
                bytes: text.to_owned(),
                classification,
                summary: summary.to_owned(),
            })
            .unwrap()
    }

    /// Stores `run` and returns its id.
    pub(crate) fn run(&mut self, run: RunSpec) -> u64 {
        let id = self.store.next_run_id().unwrap();
        self.clock += 60_000_000;
        let stored = StoredRun {
            format: crate::commands::playground::store::FORMAT,
            id,
            label: run.label,
            entry: run.entry,
            client: None,
            tool_call_id: None,
            idempotency_key: None,
            test_of: run.test_of.map(|run| RunLink {
                run: Some(run),
                execution_id: format!("exec-{run}"),
            }),
            started_at_micros: run.started_at.unwrap_or(self.clock),
            wall_ms: 12,
            dispatched: true,
            error: run.error,
            result: run.result,
            console: run.console,
            usage: ExecutionUsage::default(),
            decisions_dropped: 0,
            calls_dropped: 0,
            recording: RecordedRun {
                execution_id: format!("exec-{id}"),
                blueprint_name: "billing".into(),
                blueprint_hash: None,
                blueprint_version: run.version.map(version_tag),
                code: Some("function main(): number { return 1; }".into()),
                session_id: run.session,
                variables: run.variables,
                decisions: run.decisions,
                calls: run.calls,
                log_truncated: false,
                mcp_catalog: None,
            },
            test_report: run.test_report,
        };
        self.store.write_run(&stored).unwrap();
        id
    }

    pub(crate) fn reader(self) -> (Reader, tempfile::TempDir) {
        self.reader_with(Page::default(), Vec::new())
    }

    pub(crate) fn reader_with(
        self,
        page: Page,
        closure: Vec<ClosureEntry>,
    ) -> (Reader, tempfile::TempDir) {
        (Reader::for_tests(self.store, page, closure), self._dir)
    }
}

/// A run to store; [`RunSpec::new`] is a completed stand-in run under no version.
pub(crate) struct RunSpec {
    pub(crate) label: String,
    pub(crate) entry: String,
    pub(crate) session: Option<String>,
    pub(crate) variables: BTreeMap<String, String>,
    pub(crate) version: Option<u64>,
    pub(crate) decisions: Vec<DecisionRecord>,
    pub(crate) calls: Vec<CallRecord>,
    pub(crate) error: Option<StoredError>,
    pub(crate) result: Option<String>,
    pub(crate) console: String,
    pub(crate) test_of: Option<u64>,
    pub(crate) started_at: Option<u64>,
    pub(crate) test_report: Option<StoredTestReport>,
}

impl RunSpec {
    pub(crate) fn new(decisions: Vec<DecisionRecord>) -> Self {
        Self {
            label: "stand-in".into(),
            entry: "session".into(),
            session: Some("s-1".into()),
            variables: BTreeMap::from([("customerId".into(), "cus_northwind".into())]),
            version: None,
            calls: decisions
                .iter()
                .map(|decision| call(decision.call_index, &decision.capability))
                .collect(),
            decisions,
            error: None,
            result: Some("\"done\"".into()),
            console: String::new(),
            test_of: None,
            started_at: None,
            test_report: None,
        }
    }

    pub(crate) fn version(mut self, version: u64) -> Self {
        self.version = Some(version);
        self
    }

    pub(crate) fn session(mut self, session: &str) -> Self {
        self.session = Some(session.into());
        self
    }

    pub(crate) fn label(mut self, label: &str) -> Self {
        self.label = label.into();
        self
    }
}

pub(crate) fn call(call_index: u64, capability: &str) -> CallRecord {
    CallRecord {
        call_index,
        caller: "main".into(),
        capability: capability.into(),
        started_micros: call_index * 100,
        ended_micros: Some(call_index * 100 + 10),
        outcome: Some(CallOutcome::Returned),
        line: None,
        request: None,
        response: None,
        usage: None,
    }
}

fn record(call_index: u64, caller: &str, capability: &str, context: Value) -> DecisionRecord {
    DecisionRecord {
        call_index,
        seq: 1,
        at_micros: call_index * 100,
        caller: caller.into(),
        capability: capability.into(),
        context,
        context_truncated: false,
        context_digest: 1,
        allowed: true,
        action: DecisionAction::Allow,
        cause: DecisionCause::Default { caller_block: true },
        near_misses: Vec::new(),
        source: "policy".into(),
        rule: None,
        reason: None,
        entry_path: EntryPath::GatedOp,
        line: None,
        filtered: false,
        payload_dropped: false,
    }
}

/// `caller` allowed `capability` by its rule at `index`, named `name`.
pub(crate) fn allowed_by(
    call_index: u64,
    caller: &str,
    capability: &str,
    context: Value,
    index: usize,
    name: Option<&str>,
) -> DecisionRecord {
    let mut decision = record(call_index, caller, capability, context);
    decision.cause = DecisionCause::Rule(RuleCitation {
        caller: caller.into(),
        index,
        name: name.map(str::to_owned),
    });
    decision.rule = Some(index);
    decision
}

/// `caller` allowed `capability` by the default.
pub(crate) fn allowed_by_default(
    call_index: u64,
    caller: &str,
    capability: &str,
    context: Value,
) -> DecisionRecord {
    record(call_index, caller, capability, context)
}

/// A charges listing for `customer` allowed by the starter rule.
pub(crate) fn charges_allowed(call_index: u64, customer: &str) -> DecisionRecord {
    allowed_by(
        call_index,
        "main",
        "acme.com/charges.list",
        json!({ "customerId": customer }),
        0,
        Some("charges-for-signed-in-customer"),
    )
}

/// A charges listing for `customer` denied by the default, the starter rule a near miss
/// that needed `cus_northwind`.
pub(crate) fn charges_denied(call_index: u64, customer: &str) -> DecisionRecord {
    let mut decision = record(
        call_index,
        "main",
        "acme.com/charges.list",
        json!({ "customerId": customer }),
    );
    decision.allowed = false;
    decision.action = DecisionAction::Deny;
    decision.near_misses = vec![NearMissRecord {
        rule: RuleCitation {
            caller: "main".into(),
            index: 0,
            name: Some("charges-for-signed-in-customer".into()),
        },
        filter: "customerId == ${vars.customerId}".into(),
        failures: vec![FailureRecord {
            comparison: "customerId == ${vars.customerId}".into(),
            actual: Some(json!(customer)),
            expected: Some("cus_northwind".into()),
            reason: FailureReasonRecord::NotSatisfied,
            negated: false,
        }],
    }];
    decision
}
