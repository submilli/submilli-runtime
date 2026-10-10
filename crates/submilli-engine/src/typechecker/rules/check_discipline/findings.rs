//! The warnings a body earns, one for each root cause.
//!
//! Only a value that reaches `check()` earns one: the caller chooses every
//! other value outright, so reading it again grants nothing.

use std::collections::{BTreeMap, BTreeSet};

use super::checked::{Access, Checked, ReachKind};
use super::origin::{CallerValue, Origin, ReadKey};
use crate::{Diagnostic, Severity, Span};

const CALLER_CAN_CHANGE_IT: &str =
    "a value the caller supplies can change after `check()` approves it";
const CALLER_CODE_RUNS: &str =
    "the caller's function runs the caller's code, which can change the other arguments";

/// What a body does with a caller-supplied value besides reading it once.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Usage {
    /// Holds the name of the callee.
    PassedTo(String),
    /// Passed to a callee that has no name: the result of an expression.
    PassedToFunctionValue,
    CheckContext,
    MethodCalled(String),
    Called,
    /// Holds what the value is stored in, as the message names it.
    Stored(String),
    Spread,
    Returned,
    Thrown,
    Operand,
    Written,
    Captured,
}

/// Why a read is reported.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum ReadProblem {
    Repeated { first: Span },
    InLoop,
    ElementsRepeated { first: Span },
}

/// Why the root of a value is the caller's, at the declaration that makes it so.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct RootNote {
    pub(super) span: Span,
    pub(super) message: String,
}

pub(super) struct Findings<'a> {
    label: &'a str,
    /// The capability argument of the body's first `check()`.
    check_anchor: Span,
    checked: &'a Checked,
    /// What each reported read yields.
    reported_reads: BTreeSet<Origin>,
    diagnostics: Vec<Diagnostic>,
    usages: BTreeMap<(Origin, UsageKind), UsageFinding>,
}

impl<'a> Findings<'a> {
    pub(super) fn new(label: &'a str, check_anchor: Span, checked: &'a Checked) -> Self {
        Self {
            label,
            check_anchor,
            checked,
            reported_reads: BTreeSet::new(),
            diagnostics: Vec::new(),
            usages: BTreeMap::new(),
        }
    }

    /// A read of `key` of `receiver`, whose getter the caller may supply.
    pub(super) fn report_read(
        &mut self,
        problem: ReadProblem,
        receiver: &CallerValue,
        key: &ReadKey,
        span: Span,
    ) {
        let read = ReportedRead {
            origin: receiver.origin.after(std::slice::from_ref(key)),
            part: key.shown_on(&receiver.shown),
            whole: &receiver.shown,
            bound: receiver.bound,
            changes: "the caller can return a different value on each read",
        };
        self.report_any_read(problem, read, span);
    }

    /// A read of a variable the package declares, which the caller's code can
    /// reassign between reads through the package's own functions.
    pub(super) fn report_variable_read(
        &mut self,
        problem: ReadProblem,
        variable: &CallerValue,
        span: Span,
    ) {
        let read = ReportedRead {
            origin: variable.origin.clone(),
            part: variable.shown.clone(),
            whole: &variable.shown,
            bound: variable.bound,
            changes: "the caller's code can reassign it between reads",
        };
        self.report_any_read(problem, read, span);
    }

    fn report_any_read(&mut self, problem: ReadProblem, read: ReportedRead<'_>, span: Span) {
        let Some(reach) = self.checked.reach(&read.origin, Access::Read) else {
            return;
        };
        if !self.reported_reads.insert(read.origin) {
            return;
        }
        let label = self.label;
        let ReportedRead {
            part,
            whole,
            bound,
            changes,
            ..
        } = read;
        let diagnostic = match problem {
            ReadProblem::Repeated { first } => warning(
                span,
                format!("`{part}` is read more than once in `{label}`, which calls `check()`"),
                &format!("Read it once into a `const`; {changes}"),
                vec![
                    (first, "first read here".to_string()),
                    (self.check_anchor, "`check()` is called here".to_string()),
                    reach,
                ],
            ),
            ReadProblem::InLoop => {
                // Another package's global is bound where it is read.
                let outside = (bound != span)
                    .then(|| (bound, format!("`{whole}` is bound here, outside the loop")));
                warning(
                    span,
                    format!("`{part}` is read inside a loop in `{label}`, which calls `check()`"),
                    "Read it once before the loop",
                    outside.into_iter().chain([reach]).collect(),
                )
            }
            ReadProblem::ElementsRepeated { first } => warning(
                span,
                format!(
                    "elements of `{whole}` are read more than once in `{label}`, which calls `check()`"
                ),
                "Iterate once with `for...of`, copying the elements into the package's own array or `Map`",
                vec![(first, "first read here".to_string()), reach],
            ),
        };
        self.diagnostics.push(diagnostic);
    }

    pub(super) fn report_usage(
        &mut self,
        value: &CallerValue,
        usage: Usage,
        span: Span,
        why: RootNote,
    ) {
        let reach = match (self.checked.reach(&value.origin, Access::Pass), &usage) {
            (Some(reach), _) => reach,
            // The value is itself an argument of `check()`.
            (None, Usage::CheckContext) => (span, ReachKind::Data.note(&value.shown)),
            (None, _) => return,
        };
        self.usages
            .entry((value.origin.clone(), UsageKind::of(&usage)))
            .and_modify(|finding| finding.more = finding.more.saturating_add(1))
            .or_insert_with(|| UsageFinding {
                span,
                shown: value.shown.clone(),
                usage,
                why,
                reach,
                more: 0,
            });
    }

    /// Reported when any of `values` reaches `check()`.
    pub(super) fn report_merged_values(&mut self, values: &[CallerValue], span: Span) {
        let Some(reach) = values
            .iter()
            .find_map(|value| self.checked.reach(&value.origin, Access::Pass))
        else {
            return;
        };
        let label = self.label;
        self.diagnostics.push(warning(
            span,
            format!(
                "a conditional expression yields more than one caller-supplied value in `{label}`, which calls `check()`"
            ),
            "Bind each value to its own `const` and use those",
            vec![reach],
        ));
    }

    pub(super) fn finish(self, out: &mut Vec<Diagnostic>) {
        let label = self.label;
        out.extend(self.diagnostics);
        out.extend(
            self.usages
                .into_values()
                .map(|finding| finding.diagnostic(label)),
        );
    }
}

/// A read to report, as messages show it.
struct ReportedRead<'a> {
    /// What the read yields.
    origin: Origin,
    /// What the read yields, as the source names it.
    part: String,
    /// The value read from, or the variable read.
    whole: &'a str,
    /// Where `whole` is bound, which a read inside a loop is told to precede.
    bound: Span,
    /// Why reading again may yield another value.
    changes: &'static str,
}

pub(super) fn warning(
    span: Span,
    message: String,
    help: &str,
    notes: Vec<(Span, String)>,
) -> Diagnostic {
    Diagnostic {
        severity: Severity::Warning,
        span,
        message,
        help: vec![help.to_string()],
        notes,
    }
}

struct UsageFinding {
    span: Span,
    shown: String,
    usage: Usage,
    why: RootNote,
    reach: (Span, String),
    /// The sites after the first.
    more: u32,
}

impl UsageFinding {
    fn diagnostic(self, label: &str) -> Diagnostic {
        let shown = &self.shown;
        let verb = self.usage.verb();
        let more = match self.more {
            0 => String::new(),
            more => format!(" (and {more} more)"),
        };
        warning(
            self.span,
            format!("caller-supplied `{shown}` {verb} in `{label}`, which calls `check()`{more}"),
            &format!("{}; {}", self.usage.advice(), self.usage.reason()),
            vec![(self.why.span, self.why.message), self.reach],
        )
    }
}

impl Usage {
    fn verb(&self) -> String {
        match self {
            Usage::PassedTo(callee) => format!("is passed to `{callee}`"),
            Usage::PassedToFunctionValue => "is passed to a function value".to_string(),
            Usage::CheckContext => "is passed to `check()` as its context".to_string(),
            Usage::MethodCalled(method) => format!("has `{method}` called on it"),
            Usage::Called => "is called".to_string(),
            Usage::Stored(target) => format!("is stored in {target}"),
            Usage::Spread => "is spread into a literal".to_string(),
            Usage::Returned => "is returned".to_string(),
            Usage::Thrown => "is thrown".to_string(),
            Usage::Operand => "is used as an operand".to_string(),
            Usage::Written => "is written to".to_string(),
            Usage::Captured => "is captured by a nested function".to_string(),
        }
    }

    fn advice(&self) -> String {
        match self {
            Usage::PassedTo(_) | Usage::PassedToFunctionValue => {
                "Read what the callee needs into `const`s and pass those".to_string()
            }
            Usage::CheckContext => {
                "Build the context from values read once into `const`s".to_string()
            }
            Usage::MethodCalled(method) => format!(
                "Read what `{method}` needs into `const`s first; copy an array or `Map` with one `for...of` into the package's own array or `Map` and call `{method}` on that"
            ),
            Usage::Called => {
                "Read everything `check()` and the request need into `const`s before calling it"
                    .to_string()
            }
            Usage::Stored(_) => "Store values read once into `const`s".to_string(),
            Usage::Spread => {
                "Build the literal from values read once into `const`s, or copy an array with one `for...of`"
                    .to_string()
            }
            Usage::Returned => "Return values read once into `const`s".to_string(),
            Usage::Thrown => {
                "Throw an error built from values read once into `const`s".to_string()
            }
            Usage::Operand => "Read the value it needs once into a `const`".to_string(),
            Usage::Written => "Build a new value instead of changing the caller's".to_string(),
            Usage::Captured => {
                "Read what the nested function needs into `const`s before it".to_string()
            }
        }
    }

    /// Why the advice matters.
    fn reason(&self) -> &'static str {
        match self {
            Usage::Called => CALLER_CODE_RUNS,
            Usage::PassedTo(_)
            | Usage::PassedToFunctionValue
            | Usage::CheckContext
            | Usage::MethodCalled(_)
            | Usage::Stored(_)
            | Usage::Spread
            | Usage::Returned
            | Usage::Thrown
            | Usage::Operand
            | Usage::Written
            | Usage::Captured => CALLER_CAN_CHANGE_IT,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum UsageKind {
    PassedTo,
    CheckContext,
    MethodCalled,
    Called,
    Stored,
    Spread,
    Returned,
    Thrown,
    Operand,
    Written,
    Captured,
}

impl UsageKind {
    fn of(usage: &Usage) -> Self {
        match usage {
            Usage::PassedTo(_) | Usage::PassedToFunctionValue => UsageKind::PassedTo,
            Usage::CheckContext => UsageKind::CheckContext,
            Usage::MethodCalled(_) => UsageKind::MethodCalled,
            Usage::Called => UsageKind::Called,
            Usage::Stored(_) => UsageKind::Stored,
            Usage::Spread => UsageKind::Spread,
            Usage::Returned => UsageKind::Returned,
            Usage::Thrown => UsageKind::Thrown,
            Usage::Operand => UsageKind::Operand,
            Usage::Written => UsageKind::Written,
            Usage::Captured => UsageKind::Captured,
        }
    }
}
