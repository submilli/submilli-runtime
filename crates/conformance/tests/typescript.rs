//! TypeScript conformance: compare what our typechecker infers against what
//! `tsc` infers for the same program.
//!
//! Every case under `typescript/` is a test from TypeScript's own conformance
//! suite, ported with the mechanical rewrites in `typescript/README.md`, and sits
//! next to two baselines `tsc` wrote for the ported program:
//!
//! - `<case>.types` — the type `tsc` infers at every expression, line by line.
//! - `<case>.errors.txt` — the errors `tsc` reports, when there are any.
//!
//! The runner typechecks the case, pairs each baseline entry with the expression
//! of the same text on the same line, and writes every place the two disagree to
//! `<case>.divergences`. That file is committed: the test fails when the current
//! divergences differ from it, in either direction, so a fixed divergence and a
//! new one both show up in review. `UPDATE_TYPESCRIPT_EXPECTED=1` rewrites it.
//!
//! Each divergence is explained in the case's `<case>.triage`, or listed in
//! `unexplained.txt` until it is; `support/triage.rs` has the rules.

#[path = "support/case_errors.rs"]
mod case_errors;
#[path = "support/triage.rs"]
mod triage;
#[path = "support/typed_reachability.rs"]
mod typed_reachability;

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use interpreter::{ExprId, Span, StmtId, Type, TypedAst, TypedExprKind, TypedStmtKind};

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

/// The files that belong to a case, beside its `.ts`.
const CASE_FILE_SUFFIXES: &[&str] = &[".types", ".errors.txt", ".divergences", ".triage"];

/// The divergences not yet explained, beside the cases.
const UNEXPLAINED: &str = "unexplained.txt";

#[test]
fn typescript_baselines() {
    let root = Path::new(ROOT).join("typescript");
    let update = std::env::var("UPDATE_TYPESCRIPT_EXPECTED").is_ok_and(|v| v != "0");
    let mut failures = orphaned_case_files(&root, update);
    let unexplained_path = root.join(UNEXPLAINED);
    let listed = triage::Unexplained::read(&unexplained_path)
        .unwrap_or_else(|e| panic!("{}: {e}", rel(&unexplained_path)));
    let mut unexplained = listed.clone();
    let ported = newly_ported_cases();

    let cases = find_cases(&root);
    let mut totals = Totals::default();
    let mut checks = String::new();
    for case in &cases {
        match compare_case(case) {
            Ok(report) => {
                totals.add(&report);
                checks.push_str(&report.render_checks(&rel(case)));
                failures.extend(check_divergences(case, &report, update));
                let name = suite_path(&root, case);
                let allow_new = update && ported.contains(&name);
                failures.extend(check_triage(
                    case,
                    &name,
                    &report,
                    &mut unexplained,
                    update,
                    allow_new,
                ));
            }
            Err(message) => failures.push(format!("--- {} ---\n{message}", rel(case))),
        }
    }
    failures.extend(unexplained_without_case(&root, &mut unexplained, update));
    if update && unexplained != listed {
        unexplained.write(&unexplained_path);
    }

    if let Ok(path) = std::env::var("TYPESCRIPT_CHECKS_OUT") {
        fs::write(&path, checks).unwrap_or_else(|e| panic!("write {path}: {e}"));
    }
    eprintln!(
        "typescript: {} case(s); {} of {} tsc types compared, {} differ; \
         {} tsc error line(s) none of ours agrees with, {} error line(s) of ours tsc does not share",
        cases.len(),
        totals.compared,
        totals.baseline_entries,
        totals.type_divergences,
        totals.missed_errors,
        totals.extra_errors,
    );
    assert!(
        failures.is_empty(),
        "\n{} TypeScript conformance failure(s):\n\n{}",
        failures.len(),
        failures.join("\n\n"),
    );
}

/// Every case under `root`, narrowed by `CONFORMANCE_FILTER` when it is set.
fn find_cases(root: &Path) -> Vec<PathBuf> {
    let mut cases = Vec::new();
    collect_files(
        root,
        &mut |p| p.extension().is_some_and(|e| e == "ts"),
        &mut cases,
    );
    cases.sort();
    if let Ok(filter) = std::env::var("CONFORMANCE_FILTER") {
        cases.retain(|p| rel(p).contains(&filter));
        assert!(
            !cases.is_empty(),
            "no case matches CONFORMANCE_FILTER={filter}"
        );
    }
    assert!(!cases.is_empty(), "no cases under {}", root.display());
    cases
}

/// A failure when `report` differs from the case's committed `.divergences`;
/// in update mode, the file is rewritten instead.
fn check_divergences(case: &Path, report: &Report, update: bool) -> Option<String> {
    let rendered = report.render();
    let expected_path = case.with_extension("divergences");
    let expected = fs::read_to_string(&expected_path).unwrap_or_default();
    if rendered == expected {
        return None;
    }
    if update {
        fs::write(&expected_path, &rendered)
            .unwrap_or_else(|e| panic!("write {}: {e}", expected_path.display()));
        return None;
    }
    Some(format!(
        "--- {} ---\ndiverges differently from its committed file \
         (rerun with UPDATE_TYPESCRIPT_EXPECTED=1 if the change is intended)\n\
         expected:\n{expected}\nactual:\n{rendered}",
        rel(&expected_path)
    ))
}

/// Failures for a case's divergences that are neither explained in its `.triage` nor
/// listed in `unexplained.txt`, and for explanations and listings gone stale. In update
/// mode, stale listings are dropped, and, when `allow_new`, every divergence the
/// `.triage` doesn't explain is listed.
fn check_triage(
    case: &Path,
    name: &str,
    report: &Report,
    unexplained: &mut triage::Unexplained,
    update: bool,
    allow_new: bool,
) -> Vec<String> {
    let triage_path = case.with_extension("triage");
    let explained = match triage::read_explained(&triage_path) {
        Ok(explained) => explained,
        Err(e) => return vec![format!("--- {} ---\n{e}", rel(&triage_path))],
    };
    let divergent = report.divergent();
    if update {
        let not_explained: BTreeSet<_> = divergent.difference(&explained).copied().collect();
        let listed = if allow_new {
            not_explained
        } else {
            unexplained
                .of_case(name)
                .intersection(&not_explained)
                .copied()
                .collect()
        };
        unexplained.set_case(name, &listed);
    }
    triage::check_case(&divergent, &explained, &unexplained.of_case(name))
        .into_iter()
        .map(|failure| format!("--- {} ---\n{failure}", rel(&triage_path)))
        .collect()
}

/// Listings in `unexplained.txt` of a case that no longer exists; update mode drops
/// them.
fn unexplained_without_case(
    root: &Path,
    unexplained: &mut triage::Unexplained,
    update: bool,
) -> Vec<String> {
    let mut failures = Vec::new();
    for name in unexplained.cases() {
        if root.join(&name).exists() {
            continue;
        }
        if update {
            unexplained.set_case(&name, &BTreeSet::new());
        } else {
            failures.push(format!(
                "--- {UNEXPLAINED} ---\nlists `{name}`, which is not a case: \
                 rerun with UPDATE_TYPESCRIPT_EXPECTED=1 to remove it"
            ));
        }
    }
    failures
}

/// The cases `port-suite.cjs` has just ported, whose divergences update mode may list
/// as unexplained: `TYPESCRIPT_PORTED_CASES` names a file of their paths, one per
/// line, relative to the suite root. Nothing else adds to the list.
fn newly_ported_cases() -> BTreeSet<String> {
    let Ok(path) = std::env::var("TYPESCRIPT_PORTED_CASES") else {
        return BTreeSet::new();
    };
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// A case's path relative to the suite root, as `unexplained.txt` names it.
fn suite_path(root: &Path, case: &Path) -> String {
    case.strip_prefix(root)
        .unwrap_or(case)
        .display()
        .to_string()
}

/// Baselines, `.divergences` and `.triage` left behind by a case that no longer
/// exists. They would otherwise go unnoticed, since cases are found by their `.ts`.
/// Update mode removes the generated ones; a `.triage` is written by hand, so it
/// stays a failure until someone removes it.
fn orphaned_case_files(root: &Path, update: bool) -> Vec<String> {
    let mut files = Vec::new();
    collect_files(root, &mut |p| case_of(p).is_some(), &mut files);
    let mut failures = Vec::new();
    for file in files {
        let Some(case) = case_of(&file) else {
            continue;
        };
        if case.exists() {
            continue;
        }
        let generated = !file.extension().is_some_and(|e| e == "triage");
        if update && generated {
            fs::remove_file(&file).unwrap_or_else(|e| panic!("remove {}: {e}", file.display()));
        } else {
            failures.push(format!(
                "--- {} ---\nhas no `.ts` case beside it",
                rel(&file)
            ));
        }
    }
    failures
}

/// The `.ts` a baseline, `.divergences` or `.triage` file belongs to.
fn case_of(file: &Path) -> Option<PathBuf> {
    let name = file.file_name()?.to_str()?;
    let base = CASE_FILE_SUFFIXES
        .iter()
        .find_map(|suffix| name.strip_suffix(suffix))?;
    Some(file.with_file_name(format!("{base}.ts")))
}

#[derive(Default)]
struct Totals {
    baseline_entries: usize,
    compared: usize,
    type_divergences: usize,
    missed_errors: usize,
    extra_errors: usize,
}

impl Totals {
    fn add(&mut self, report: &Report) {
        self.baseline_entries += report.baseline_entries;
        self.compared += report.compared.len();
        self.type_divergences += report.type_divergences.len();
        self.missed_errors += report.missed_errors.len();
        self.extra_errors += report.extra_errors.len();
    }
}

/// Everything one case disagrees with `tsc` about.
struct Report {
    baseline_entries: usize,
    /// The `tsc` entries compared.
    compared: Vec<ComparedEntry>,
    /// The lines whose errors were compared: where `tsc` reports one, or where we
    /// report one that could agree.
    error_lines: BTreeSet<usize>,
    type_divergences: Vec<TypeDivergence>,
    /// `tsc`'s errors that none of ours agrees with.
    missed_errors: Vec<MissedError>,
    /// Our errors on lines where `tsc` reports none, or where ours is about syntax
    /// or a feature we lack.
    extra_errors: Vec<ExtraError>,
}

impl Report {
    fn render(&self) -> String {
        let mut out = format!(
            "types: {} of {} tsc entries compared, {} differ\n",
            self.compared.len(),
            self.baseline_entries,
            self.type_divergences.len(),
        );
        for d in &self.type_divergences {
            out.push_str(&format!(
                "\nline {}: {}\n  tsc:  {}\n  ours: {}\n",
                d.line, d.text, d.tsc, d.ours
            ));
        }
        if !self.missed_errors.is_empty() || !self.extra_errors.is_empty() {
            out.push_str("\nerrors:\n");
        }
        for MissedError {
            error: e,
            we_reject,
        } in &self.missed_errors
        {
            let our_note = if *we_reject {
                "we reject the line for another reason"
            } else {
                "we accept"
            };
            out.push_str(&format!(
                "line {}: tsc {}: {} ({our_note})\n",
                e.line, e.code, e.message
            ));
        }
        for e in &self.extra_errors {
            let tsc_note = if e.tsc_rejects {
                "tsc rejects the line for another reason"
            } else {
                "tsc accepts"
            };
            out.push_str(&format!(
                "line {}: ours: {} ({tsc_note})\n",
                e.line, e.message
            ));
        }
        out
    }

    /// Each line that diverges, and how.
    fn divergent(&self) -> BTreeSet<triage::Divergence> {
        let types = self
            .type_divergences
            .iter()
            .map(|d| (d.line, triage::Kind::Type));
        let missed = self
            .missed_errors
            .iter()
            .map(|m| (m.error.line, triage::Kind::Missed));
        let extra = self
            .extra_errors
            .iter()
            .map(|e| (e.line, triage::Kind::Extra));
        types.chain(missed).chain(extra).collect()
    }

    /// Every check the case makes, one per line, for `TYPESCRIPT_CHECKS_OUT`:
    /// `<case>\t<line>\ttype\t<entry text>\t<tsc type>` for a type compared, and
    /// `<case>\t<line>\terror\t\t` for a line whose errors were compared.
    fn render_checks(&self, case: &str) -> String {
        let types = self
            .compared
            .iter()
            .map(|e| format!("{case}\t{}\ttype\t{}\t{}\n", e.line, e.text, e.tsc));
        let errors = self
            .error_lines
            .iter()
            .map(|line| format!("{case}\t{line}\terror\t\t\n"));
        types.chain(errors).collect()
    }
}

/// A `tsc` entry that was compared with ours.
struct ComparedEntry {
    line: usize,
    text: String,
    tsc: String,
}

struct MissedError {
    error: BaselineError,
    /// We report an error on the line too, but ours lacks support, so the two reject
    /// it for different reasons.
    we_reject: bool,
}

struct ExtraError {
    line: usize,
    message: String,
    /// `tsc` reports an error on the line too, but ours lacks support, so the two
    /// reject it for different reasons.
    tsc_rejects: bool,
}

struct TypeDivergence {
    line: usize,
    text: String,
    tsc: String,
    ours: String,
}

/// An expression's (whitespace-collapsed) source text and its rendered type.
struct Entry {
    text: String,
    ty: String,
}

/// Entries keyed by the 1-based line they start on, in source order.
type EntriesByLine = BTreeMap<usize, Vec<Entry>>;

fn compare_case(case: &Path) -> Result<Report, String> {
    let source = fs::read_to_string(case).map_err(|e| format!("read: {e}"))?;
    let baseline = read_baseline_types(&case.with_extension("types"), &source)?;
    let (typed, errors) = case_errors::case_errors(&source);
    let ours = entries_by_line(&typed, &source);
    let our_errors = errors_by_line(errors);
    let tsc_errors = read_baseline_errors(case);
    let agreed_lines = agreed_lines(&tsc_errors, &our_errors);
    let our_error_lines: BTreeSet<usize> = our_errors.keys().copied().collect();
    let (compared, type_divergences) =
        compare_types(&baseline, &ours, &our_error_lines, &agreed_lines);
    let error_lines = tsc_errors
        .iter()
        .map(|e| e.line)
        .chain(
            our_errors
                .iter()
                .filter(|(_, e)| e.can_agree())
                .map(|(&line, _)| line),
        )
        .collect();
    let (missed_errors, extra_errors) = compare_errors(tsc_errors, &our_errors, &agreed_lines);
    Ok(Report {
        baseline_entries: baseline.values().map(Vec::len).sum(),
        compared,
        error_lines,
        type_divergences,
        missed_errors,
        extra_errors,
    })
}

/// The `tsc` entries compared, with their lines, and those whose type differs from
/// ours.
///
/// Skips a pair where a side's type is its error recovery, which says nothing about
/// inference: ours holding `<error>` on a line where we report an error (the error
/// is recorded already), and `tsc`'s `any` on a line where the two errors agree.
fn compare_types(
    baseline: &EntriesByLine,
    ours: &EntriesByLine,
    our_error_lines: &BTreeSet<usize>,
    agreed_lines: &BTreeSet<usize>,
) -> (Vec<ComparedEntry>, Vec<TypeDivergence>) {
    let error_type = Type::Error.to_string();
    let is_recovery = |line: &usize, tsc_ty: &str, our_ty: &str| {
        (our_ty.contains(&error_type) && our_error_lines.contains(line))
            || (tsc_ty == "any" && agreed_lines.contains(line))
    };
    let mut compared = Vec::new();
    let mut divergences = Vec::new();
    for (line, entries) in baseline {
        let ours_on_line = ours.get(line).map_or(&[][..], Vec::as_slice);
        for (text, tsc_types) in group_by_text(entries) {
            if is_literal(text) {
                continue;
            }
            let our_types: Vec<&str> = ours_on_line
                .iter()
                .filter(|entry| entry.text == text)
                .map(|entry| entry.ty.as_str())
                .collect();
            // Pair occurrences in order only when both sides saw the same number of
            // them; otherwise a declaration name or a rewritten token would shift
            // every pairing after it.
            if our_types.len() != tsc_types.len() {
                continue;
            }
            for (tsc_ty, our_ty) in tsc_types.into_iter().zip(our_types) {
                if is_recovery(line, tsc_ty, our_ty) {
                    continue;
                }
                compared.push(ComparedEntry {
                    line: *line,
                    text: text.to_string(),
                    tsc: tsc_ty.to_string(),
                });
                if !same_type(text, tsc_ty, our_ty) {
                    divergences.push(TypeDivergence {
                        line: *line,
                        text: text.to_string(),
                        tsc: tsc_ty.to_string(),
                        ours: our_ty.to_string(),
                    });
                }
            }
        }
    }
    (compared, divergences)
}

/// The lines where our error agrees with `tsc`'s: both reject the line, and ours
/// could agree.
fn agreed_lines(
    tsc_errors: &[BaselineError],
    our_errors: &BTreeMap<usize, LineError>,
) -> BTreeSet<usize> {
    tsc_errors
        .iter()
        .filter(|e| our_errors.get(&e.line).is_some_and(LineError::can_agree))
        .map(|e| e.line)
        .collect()
}

/// `tsc`'s errors that none of ours agrees with, and ours that agree with none of
/// `tsc`'s. Ours agrees with an error `tsc` reports on the same line, unless ours
/// lacks support: then the two reject the line for different reasons.
fn compare_errors(
    tsc_errors: Vec<BaselineError>,
    our_errors: &BTreeMap<usize, LineError>,
    agreed: &BTreeSet<usize>,
) -> (Vec<MissedError>, Vec<ExtraError>) {
    let extra = our_errors
        .iter()
        .filter_map(|(&line, ours)| {
            let tsc_rejects = tsc_errors.iter().any(|e| e.line == line);
            (!agreed.contains(&line)).then(|| ExtraError {
                line,
                message: ours.message.clone(),
                tsc_rejects,
            })
        })
        .collect();
    let missed = tsc_errors
        .into_iter()
        .filter(|e| !agreed.contains(&e.line))
        .map(|error| MissedError {
            we_reject: our_errors.contains_key(&error.line),
            error,
        })
        .collect();
    (missed, extra)
}

/// Our type for every expression and every name a declaration or assignment
/// binds.
fn entries_by_line(typed: &TypedAst, source: &str) -> EntriesByLine {
    let reachable = typed_reachability::Reachable::collect(typed);
    let expressions = (0..typed.exprs_len()).filter_map(|i| {
        let id = ExprId(i as u32);
        let e = typed.try_expr(id).unwrap();
        let text = span_text(source, e.span)?;
        Some((
            e.span,
            text,
            Claim::of(&e.kind, i, text, reachable.expressions.contains(&id)),
            e.ty.clone(),
        ))
    });
    let names = bindings(typed, &reachable)
        .into_iter()
        .filter_map(|(span, ty, retained)| {
            Some((span, span_text(source, span)?, Claim::Binding(retained), ty))
        });
    let mut nodes: Vec<(Span, &str, Claim, Type)> = expressions.chain(names).collect();
    nodes.sort_by_key(|(span, _, claim, _)| (span.start, Reverse(span.end), Reverse(*claim)));
    nodes.dedup_by(|a, b| a.0.start == b.0.start && a.0.end == b.0.end);

    let mut by_line = EntriesByLine::new();
    for (span, text, _, ty) in nodes {
        by_line
            .entry(line_of(source, span.start as usize))
            .or_default()
            .push(Entry {
                text: collapse_whitespace(text),
                ty: ty.to_string(),
            });
    }
    by_line
}

/// Which of the nodes sharing a span is the one the source wrote; the greatest
/// wins. The typechecker gives each node it synthesizes (a default argument, a
/// rest parameter's array, the comparisons a `switch` expands to, a narrowed or
/// captured read) the span of the construct it belongs to. Within a category,
/// prefer retained nodes over abandoned inference retries. Keep unreachable
/// candidates as a fallback for source expressions erased by constant folding.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Claim {
    /// A variable reference whose span is not a name: a closure's capture.
    SynthesizedReference,
    /// Any other expression. The last one created at a span is the construct
    /// itself, created after the pieces synthesized for it.
    Expression(bool, usize),
    /// A variable reference at a name. The first one created is the read the
    /// source wrote; a narrowed copy comes after it.
    Reference(bool, Reverse<usize>),
    /// A declared or assigned name, typed by what the binding holds.
    Binding(bool),
}

impl Claim {
    fn of(kind: &TypedExprKind, index: usize, text: &str, retained: bool) -> Self {
        let is_reference = matches!(
            kind,
            TypedExprKind::LocalRef { .. }
                | TypedExprKind::LocalNarrowRef { .. }
                | TypedExprKind::GlobalRef { .. }
                | TypedExprKind::FunctionRef { .. }
        );
        match (is_reference, is_identifier(text)) {
            (true, true) => Claim::Reference(retained, Reverse(index)),
            (true, false) => Claim::SynthesizedReference,
            (false, _) => Claim::Expression(retained, index),
        }
    }
}

/// Each name a declaration or assignment binds, with the binding's type. `tsc`
/// types an assignment's target, like a declaration's name, by what the binding
/// holds, not by the value being assigned.
fn bindings(
    typed: &TypedAst,
    reachable: &typed_reachability::Reachable,
) -> Vec<(Span, Type, bool)> {
    let mut out: Vec<(Span, Type, bool)> = typed
        .globals
        .iter()
        .map(|g| (g.name.span, g.ty.clone(), true))
        .collect();
    for function in &typed.functions {
        out.extend(
            function
                .params
                .iter()
                .map(|p| (p.name.span, p.ty.clone(), true)),
        );
    }
    for i in 0..typed.stmts_len() {
        let id = StmtId(i as u32);
        let retained = reachable.statements.contains(&id);
        match &typed.try_stmt(id).unwrap().kind {
            TypedStmtKind::Let { name, ty, .. } | TypedStmtKind::Const { name, ty, .. } => {
                out.push((name.span, ty.clone(), retained));
            }
            TypedStmtKind::ForOf {
                name, element_ty, ..
            } => out.push((name.span, element_ty.clone(), retained)),
            TypedStmtKind::AssignLocal {
                ident, target_ty, ..
            }
            | TypedStmtKind::AssignGlobal {
                ident, target_ty, ..
            } => out.push((ident.span, target_ty.clone(), retained)),
            _ => {}
        }
    }
    out
}

/// `tsc`'s entries from a `.types` baseline, keyed by line in the case.
///
/// The baseline echoes the case without its `// @option:` lines, each source line
/// followed by `>text : type` entries for the expressions starting on it, so the
/// n-th non-blank line of the baseline is the n-th non-blank, non-option line of
/// the case.
fn read_baseline_types(path: &Path, source: &str) -> Result<EntriesByLine, String> {
    let baseline = fs::read_to_string(path).map_err(|e| {
        format!(
            "read {}: {e}; write it with typescript-baselines/write-baselines.cjs",
            rel(path)
        )
    })?;
    let case_lines = countable_line_numbers(source);
    let lines: Vec<&str> = baseline.lines().collect();
    let body_start = lines
        .iter()
        .position(|l| l.starts_with("=== "))
        .map_or(lines.len(), |i| i + 1);

    let mut entries = EntriesByLine::new();
    let mut source_index: Option<usize> = None;
    let mut after_entry = false;
    for (i, line) in lines.iter().enumerate().skip(body_start) {
        // Only the line under an entry is skipped as its caret line: an echoed
        // source line can look like one, inside a template literal or a comment.
        if std::mem::take(&mut after_entry) && is_caret_line(line) {
            continue;
        }
        // A `>` line is an entry only with its caret line under it; otherwise it is
        // an echoed source line that happens to start with `>`.
        let entry = line
            .strip_prefix('>')
            .zip(lines.get(i + 1).filter(|next| is_caret_line(next)))
            .and_then(|(entry, caret_line)| split_entry(entry, &caret_line[1..]));
        let Some(entry) = entry else {
            if !line.trim().is_empty() {
                source_index = Some(source_index.map_or(0, |n| n + 1));
            }
            continue;
        };
        after_entry = true;
        if let Some(&line_no) = source_index.and_then(|n| case_lines.get(n)) {
            entries.entry(line_no).or_default().push(entry);
        }
    }
    Ok(entries)
}

/// The line under an entry: `>`, a space per UTF-16 unit of the text and one
/// more, then `: ` and the carets under the type.
fn is_caret_line(line: &str) -> bool {
    line.strip_prefix('>')
        .filter(|rest| rest.starts_with(' '))
        .and_then(|rest| rest.trim_start_matches(' ').strip_prefix(": "))
        .is_some_and(|carets| carets.chars().all(|c| matches!(c, '^' | ' ')))
}

/// The 1-based numbers of the case's lines a baseline echoes: the non-blank lines
/// that are not `// @option:` lines. `tsc` drops a leading byte-order mark.
fn countable_line_numbers(source: &str) -> Vec<usize> {
    source
        .strip_prefix('\u{feff}')
        .unwrap_or(source)
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty() && !is_option_line(l))
        .map(|(i, _)| i + 1)
        .collect()
}

/// Splits a `>text : type` entry at the column of the ` : ` in the caret line
/// under it — the only reliable split, since the text can itself contain ` : `.
/// That column counts UTF-16 units of the text, as `tsc` measures strings.
fn split_entry(entry: &str, caret_line: &str) -> Option<Entry> {
    let text_units = caret_line.find(" : ")?;
    let mut units = 0;
    let colon = entry
        .char_indices()
        .find_map(|(byte, c)| {
            let at = (units == text_units).then_some(byte);
            units += c.len_utf16();
            at
        })
        .filter(|&byte| entry[byte..].starts_with(" : "))?;
    Some(Entry {
        text: collapse_whitespace(&entry[..colon]),
        ty: entry[colon + " : ".len()..].to_string(),
    })
}

struct BaselineError {
    line: usize,
    code: String,
    message: String,
}

/// `tsc`'s errors in this case, one `file(line,col): error TSn: message` line each.
fn read_baseline_errors(case: &Path) -> Vec<BaselineError> {
    let Ok(text) = fs::read_to_string(case.with_extension("errors.txt")) else {
        return Vec::new();
    };
    let file_name = case
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let mut errors: Vec<BaselineError> = Vec::new();
    for line in text.lines() {
        let Some(rest) = line
            .strip_prefix(file_name)
            .and_then(|r| r.strip_prefix('('))
        else {
            continue;
        };
        let Some((position, rest)) = rest.split_once("): error ") else {
            continue;
        };
        let Some((code, message)) = rest.split_once(": ") else {
            continue;
        };
        let Some(line_no) = position.split(',').next().and_then(|n| n.parse().ok()) else {
            continue;
        };
        if errors.iter().any(|e| e.line == line_no) {
            continue;
        }
        errors.push(BaselineError {
            line: line_no,
            code: code.to_string(),
            message: message.to_string(),
        });
    }
    errors
}

/// The error of ours that stands for its line.
struct LineError {
    message: String,
    unsupported: bool,
}

impl LineError {
    fn can_agree(&self) -> bool {
        !self.unsupported
    }
}

/// One of our errors on each line: the first that lacks support, else the first.
fn errors_by_line(errors: Vec<case_errors::CaseError>) -> BTreeMap<usize, LineError> {
    let mut lines: BTreeMap<usize, LineError> = BTreeMap::new();
    for e in errors {
        let replace = lines
            .get(&e.line)
            .is_none_or(|kept| kept.can_agree() && e.support == case_errors::Support::Lacking);
        if replace {
            lines.insert(
                e.line,
                LineError {
                    message: e.message,
                    unsupported: e.support == case_errors::Support::Lacking,
                },
            );
        }
    }
    lines
}

fn group_by_text(entries: &[Entry]) -> Vec<(&str, Vec<&str>)> {
    let mut groups: Vec<(&str, Vec<&str>)> = Vec::new();
    for entry in entries {
        match groups.iter_mut().find(|(text, _)| *text == entry.text) {
            Some((_, types)) => types.push(&entry.ty),
            None => groups.push((&entry.text, vec![&entry.ty])),
        }
    }
    groups
}

/// A literal written in the source. `tsc` gives each one its own literal type
/// (`1`, `"a"`, `true`) and widens it where it is bound; we widen at the literal
/// itself. Only the bound type is observable, and that is compared at the
/// binding, so the literal's own entry is skipped.
fn is_literal(text: &str) -> bool {
    matches!(text, "true" | "false" | "null")
        || is_signed_number_literal(text)
        || is_string_literal(text)
}

/// A numeric literal with an optional sign, which `tsc` may print spaced from it:
/// `-1`, `+1`, `- 10`.
fn is_signed_number_literal(text: &str) -> bool {
    is_number_literal(text.strip_prefix(['-', '+']).map_or(text, str::trim_start))
}

/// One numeric literal token: `1`, `1.5`, `.5`, `1e-3`, `0x10`, `1_000`, `10n`.
/// Not `NaN` or `Infinity`, which are names, and not `1.5.toFixed`.
fn is_number_literal(text: &str) -> bool {
    // `_` separates digits but can't lead, or `_1` would read as a number.
    if !text
        .strip_prefix('.')
        .unwrap_or(text)
        .starts_with(|c: char| c.is_ascii_digit())
    {
        return false;
    }
    let rest = text.strip_suffix('n').unwrap_or(text);
    for prefix in ["0x", "0X", "0o", "0O", "0b", "0B"] {
        if let Some(body) = rest.strip_prefix(prefix) {
            let radix = match prefix.as_bytes()[1].to_ascii_lowercase() {
                b'x' => 16,
                b'o' => 8,
                _ => 2,
            };
            let (len, after) = digits(body, radix);
            return len > 0 && after.is_empty();
        }
    }
    let (whole, rest) = digits(rest, 10);
    let (fraction, rest) = match rest.strip_prefix('.') {
        Some(after_dot) => digits(after_dot, 10),
        None => (0, rest),
    };
    if whole + fraction == 0 {
        return false;
    }
    let Some(exponent) = rest.strip_prefix(['e', 'E']) else {
        return rest.is_empty();
    };
    let (len, after) = digits(exponent.strip_prefix(['+', '-']).unwrap_or(exponent), 10);
    len > 0 && after.is_empty()
}

/// How many leading digits of `radix` (or `_` separators) `s` has, and what follows.
fn digits(s: &str, radix: u32) -> (usize, &str) {
    let end = s
        .find(|c: char| !(c.is_digit(radix) || c == '_'))
        .unwrap_or(s.len());
    (end, &s[end..])
}

/// One quoted string with nothing after its closing quote, so `"a" + "b"` is not
/// one; a template only when it has no `${}`.
fn is_string_literal(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(quote @ ('"' | '\'' | '`')) = chars.next() else {
        return false;
    };
    let body = chars.as_str();
    let Some(close) = closing_quote(body, quote) else {
        return false;
    };
    close + quote.len_utf8() == body.len() && !(quote == '`' && body.contains("${"))
}

/// The byte offset in `body` of the first unescaped `quote`.
fn closing_quote(body: &str, quote: char) -> Option<usize> {
    let mut escaped = false;
    for (i, c) in body.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            c if c == quote => return Some(i),
            _ => {}
        }
    }
    None
}

fn is_identifier(text: &str) -> bool {
    text.starts_with(|c: char| c.is_alphabetic() || matches!(c, '_' | '$'))
        && text
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '$'))
}

/// Whether `tsc`'s and our types for the expression `text` are the same type.
///
/// `tsc` types `this` in an instance member as the polymorphic `this` type, where
/// we give it the enclosing class. Submilli can't write `: this`, so nothing can
/// tell the two apart; the type text alone doesn't name the class, hence a rule
/// on the expression. Ours must still be a class's name: `unknown`, a union or
/// an error there is a real difference.
fn same_type(text: &str, tsc_ty: &str, our_ty: &str) -> bool {
    if text == "this" && tsc_ty == "this" && names_a_class(our_ty) {
        return true;
    }
    normalize_type(tsc_ty) == normalize_type(our_ty)
}

/// Whether `ty` is a single named type, `Name` or `Name<…>`, that isn't a
/// built-in one: what we print for a class.
fn names_a_class(ty: &str) -> bool {
    let name = ty.split_once('<').map_or(ty, |(name, _)| name);
    let arguments_closed = !ty.contains('<') || ty.ends_with('>');
    let is_builtin = matches!(
        name,
        "unknown" | "never" | "null" | "void" | "number" | "string" | "boolean" | "bigint"
    );
    is_identifier(name) && arguments_closed && !is_builtin
}

/// A type's text in a canonical form, so that two spellings of the same type
/// compare equal: union members and object fields sorted, parameter names and
/// binding patterns dropped (`tsc` prints the declared name, we print `arg0`),
/// method signatures read as function-typed fields, and no `;` before an
/// object's `}`. A few spellings only `tsc` uses read as ours; each is noted
/// where it is read (`canonical_generic`, `drop_literals_beside_their_base`,
/// `optional_field_type`).
/// Text the reader does not understand is compared as written.
fn normalize_type(ty: &str) -> String {
    let text = collapse_whitespace(ty);
    let mut reader = TypeText {
        text: &text,
        pos: 0,
    };
    match reader.union() {
        Some(canonical) if reader.pos == text.len() => canonical.text,
        _ => text,
    }
}

/// A recursive-descent reader over the type syntax both printers produce. Each
/// method returns the canonical text of what it read.
struct TypeText<'a> {
    text: &'a str,
    pos: usize,
}

/// A type as `TypeText` reads it, in canonical text. A function type is marked,
/// because as a union member it needs parentheses: `(() => A) | B` is not
/// `() => A | B`.
struct CanonicalType {
    text: String,
    is_function: bool,
}

impl CanonicalType {
    fn plain(text: String) -> Self {
        CanonicalType {
            text,
            is_function: false,
        }
    }
}

/// What an object member's type reads as.
enum FieldType {
    Typed(String),
    /// An optional field typed only `undefined`: `tsc`'s `b?: undefined`, for a
    /// field only some members of a normalized union have. It adds nothing to
    /// the object, so it is dropped.
    Absent,
}

/// A generic type's canonical text. Three spellings only `tsc` uses read as ours:
/// - `Uint8Array<ArrayBuffer>`: `tsc` names the buffer behind it, which ours has
///   no choice of;
/// - `Record<string, V>`: defined as the index signature we print;
/// - `ArrayIterator<T>`: `tsc`'s name for an array's iterator, which is our
///   `Iterator<T>`.
fn canonical_generic(name: &str, args: &[String]) -> String {
    match (name, args) {
        ("Uint8Array", [buffer]) if buffer == "ArrayBuffer" || buffer == "ArrayBufferLike" => {
            name.to_string()
        }
        ("Record", [key, value]) if key == "string" => format!("{{ [key: string]: {value} }}"),
        ("ArrayIterator", _) => format!("Iterator<{}>", args.join(", ")),
        _ => format!("{name}<{}>", args.join(", ")),
    }
}

/// Drops each union member that is a literal of a base type the union also has:
/// `string | "a"` is `string`. `tsc` prints the reduced union; ours may not
/// reduce it. A bigint literal (`1n`) is not a `number`.
fn drop_literals_beside_their_base(members: &mut Vec<String>) {
    let has = |base: &str| members.iter().any(|m| m == base);
    let (has_string, has_number, has_bigint, has_boolean) =
        (has("string"), has("number"), has("bigint"), has("boolean"));
    members.retain(|m| {
        let is_numeric = is_signed_number_literal(m);
        let is_bigint = is_numeric && m.ends_with('n');
        let absorbed = has_string && m.starts_with('"')
            || has_number && is_numeric && !is_bigint
            || has_bigint && is_bigint
            || has_boolean && matches!(m.as_str(), "true" | "false");
        !absorbed
    });
}

impl TypeText<'_> {
    fn union(&mut self) -> Option<CanonicalType> {
        let mut members = vec![self.postfix()?];
        while self.eat(" | ") {
            members.push(self.postfix()?);
        }
        Self::join_members(members)
    }

    /// An optional field's type, without the `undefined` its `?` already implies:
    /// `a?: T | undefined` is `a?: T` without `exactOptionalPropertyTypes`.
    fn optional_field_type(&mut self) -> Option<FieldType> {
        let mut members = Vec::new();
        loop {
            if !self.eat_word("undefined") {
                members.push(self.postfix()?);
            }
            if !self.eat(" | ") {
                break;
            }
        }
        if members.is_empty() {
            return Some(FieldType::Absent);
        }
        Some(FieldType::Typed(Self::join_members(members)?.text))
    }

    fn join_members(mut members: Vec<CanonicalType>) -> Option<CanonicalType> {
        if members.len() == 1 {
            return members.pop();
        }
        let mut texts: Vec<String> = members
            .into_iter()
            .map(|m| {
                if m.is_function {
                    format!("({})", m.text)
                } else {
                    m.text
                }
            })
            .collect();
        texts.sort();
        texts.dedup();
        drop_literals_beside_their_base(&mut texts);
        Some(CanonicalType::plain(texts.join(" | ")))
    }

    fn union_text(&mut self) -> Option<String> {
        self.union().map(|ty| ty.text)
    }

    fn postfix(&mut self) -> Option<CanonicalType> {
        // `readonly` applies to the whole postfix type after it: `readonly T[][]`.
        if self.eat("readonly ") {
            let operand = self.postfix()?;
            return Some(CanonicalType::plain(format!("readonly {}", operand.text)));
        }
        let mut ty = self.primary()?;
        while self.eat("[]") {
            ty = CanonicalType::plain(format!("({})[]", ty.text));
        }
        Some(ty)
    }

    /// A tuple element. Its label is dropped, like a parameter name: `tsc` prints
    /// `[x: number]`, we print `[number]`, and labels do not change the type.
    fn tuple_element(&mut self) -> Option<String> {
        let start = self.pos;
        let optional = match self.word() {
            Some(_) if self.eat("?: ") => "?",
            Some(_) if self.eat(": ") => "",
            _ => {
                self.pos = start;
                ""
            }
        };
        Some(format!("{}{optional}", self.union_text()?))
    }

    fn primary(&mut self) -> Option<CanonicalType> {
        if self.rest().starts_with('(') {
            return self.function_or_group();
        }
        if self.eat("{") {
            return self.object().map(CanonicalType::plain);
        }
        if self.eat("[") {
            let elements = self.list("]", Self::tuple_element)?;
            return Some(CanonicalType::plain(format!("[{}]", elements.join(", "))));
        }
        if self.rest().starts_with('"') {
            return self.string_literal().map(CanonicalType::plain);
        }
        let name = self.word()?;
        // The port spells `undefined` as `null`, so `tsc`'s `undefined` is ours.
        if name == "undefined" {
            return Some(CanonicalType::plain("null".to_string()));
        }
        if self.eat("<") {
            let args = self.list(">", Self::union_text)?;
            return Some(CanonicalType::plain(canonical_generic(&name, &args)));
        }
        Some(CanonicalType::plain(name))
    }

    /// `(a: T, b?: U) => R` reads as `(T, U?) => R`; anything else in
    /// parentheses is a grouped type, read as what it groups.
    fn function_or_group(&mut self) -> Option<CanonicalType> {
        let start = self.pos;
        self.eat("(");
        if let Some(params) = self.list(")", Self::parameter)
            && self.eat(" => ")
        {
            let ret = self.union_text()?;
            return Some(CanonicalType {
                text: format!("({}) => {ret}", params.join(", ")),
                is_function: true,
            });
        }
        self.pos = start + 1;
        let inner = self.union()?;
        self.eat(")").then_some(inner)
    }

    fn parameter(&mut self) -> Option<String> {
        let rest = if self.eat("...") { "..." } else { "" };
        // `tsc` prints a destructured parameter by its pattern, `([a, b]: T) => R`;
        // like a name, the pattern doesn't change the parameter's type.
        if !self.eat_binding_pattern() {
            self.word()?;
        }
        let optional = if self.eat("?") { "?" } else { "" };
        if !self.eat(": ") {
            return None;
        }
        Some(format!("{rest}{}{optional}", self.union_text()?))
    }

    fn object(&mut self) -> Option<String> {
        let mut fields = Vec::new();
        self.eat(" ");
        while !self.eat("}") {
            let readonly = if self.eat("readonly ") {
                "readonly "
            } else {
                ""
            };
            let name = self.object_member_name()?;
            let optional = if self.eat("?") { "?" } else { "" };
            if let FieldType::Typed(ty) = self.field_type(!optional.is_empty())? {
                fields.push(format!("{readonly}{name}{optional}: {ty}"));
            }
            let separated = self.eat(";") || self.eat(",");
            self.eat(" ");
            if !separated && !self.rest().starts_with('}') {
                return None;
            }
        }
        fields.sort();
        Some(if fields.is_empty() {
            "{}".to_string()
        } else {
            format!("{{ {} }}", fields.join("; "))
        })
    }

    /// A member's type after its name: `: T`, or a method signature's `(a: T): R`,
    /// which is the field `m: (a: T) => R`.
    fn field_type(&mut self, is_optional: bool) -> Option<FieldType> {
        if self.eat("(") {
            let params = self.list(")", Self::parameter)?;
            if !self.eat(": ") {
                return None;
            }
            let ret = self.union_text()?;
            return Some(FieldType::Typed(format!(
                "({}) => {ret}",
                params.join(", ")
            )));
        }
        if !self.eat(": ") {
            return None;
        }
        if is_optional {
            return self.optional_field_type();
        }
        self.union_text().map(FieldType::Typed)
    }

    /// Index parameter names are labels; their key types remain significant.
    fn object_member_name(&mut self) -> Option<String> {
        if !self.eat("[") {
            return self.word();
        }
        self.word()?;
        if !self.eat(": ") {
            return None;
        }
        let key = self.union_text()?;
        self.eat("]").then(|| format!("[key: {key}]"))
    }

    /// Items read by `item`, separated by `, `, through the closing `close`.
    fn list(
        &mut self,
        close: &str,
        mut item: impl FnMut(&mut Self) -> Option<String>,
    ) -> Option<Vec<String>> {
        let mut items = Vec::new();
        if self.eat(close) {
            return Some(items);
        }
        loop {
            items.push(item(self)?);
            if self.eat(close) {
                return Some(items);
            }
            if !self.eat(", ") {
                return None;
            }
        }
    }

    fn string_literal(&mut self) -> Option<String> {
        let rest = self.rest();
        let end = 1 + closing_quote(&rest[1..], '"')? + 1;
        let literal = rest[..end].to_string();
        self.pos += end;
        Some(literal)
    }

    /// A name, keyword, or number: everything up to the next delimiter.
    fn word(&mut self) -> Option<String> {
        let rest = self.rest();
        let len = rest
            .find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '$' | '.' | '-')))
            .unwrap_or(rest.len());
        if len == 0 {
            return None;
        }
        let word = rest[..len].to_string();
        self.pos += len;
        Some(word)
    }

    /// A balanced `[…]` or `{…}` binding pattern, skipped whole.
    fn eat_binding_pattern(&mut self) -> bool {
        let rest = self.rest();
        if !rest.starts_with(['[', '{']) {
            return false;
        }
        let mut depth = 0usize;
        for (i, c) in rest.char_indices() {
            match c {
                '[' | '{' => depth += 1,
                ']' | '}' => {
                    depth -= 1;
                    if depth == 0 {
                        self.pos += i + 1;
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    /// `word` as a whole type: not the start of a longer name (the characters
    /// `word()` reads), an array (`[]`) or a generic (`<`).
    fn eat_word(&mut self, word: &str) -> bool {
        let rest = self.rest();
        let whole = rest.strip_prefix(word).is_some_and(|after| {
            !after.starts_with(|c: char| {
                c.is_alphanumeric() || matches!(c, '_' | '$' | '.' | '-' | '[' | '<')
            })
        });
        if whole {
            self.pos += word.len();
        }
        whole
    }

    fn eat(&mut self, token: &str) -> bool {
        let matched = self.rest().starts_with(token);
        if matched {
            self.pos += token.len();
        }
        matched
    }

    fn rest(&self) -> &str {
        &self.text[self.pos..]
    }
}

fn span_text(source: &str, span: Span) -> Option<&str> {
    let (start, end) = (span.start as usize, span.end as usize);
    (start < end).then(|| source.get(start..end)).flatten()
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn line_of(source: &str, offset: usize) -> usize {
    source[..offset.min(source.len())].matches('\n').count() + 1
}

/// A `// @name: value` compiler-option line, which the baseline writer leaves out.
/// Must agree with `OPTION_LINE` in `typescript-baselines/tsc-case.cjs`:
/// `^\s*//\s*@\w+\s*:`. JavaScript's `\s` includes a byte-order mark.
fn is_option_line(line: &str) -> bool {
    let Some(rest) = trim_js_space(line)
        .strip_prefix("//")
        .and_then(|r| trim_js_space(r).strip_prefix('@'))
    else {
        return false;
    };
    let name_len = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    name_len > 0 && trim_js_space(&rest[name_len..]).starts_with(':')
}

/// `line` without the leading characters JavaScript's `\s` matches.
fn trim_js_space(line: &str) -> &str {
    line.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
}

fn collect_files(dir: &Path, keep: &mut impl FnMut(&Path) -> bool, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir({}): {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            collect_files(&path, keep, out);
        } else if keep(&path) {
            out.push(path);
        }
    }
}

fn rel(p: &Path) -> String {
    p.strip_prefix(ROOT).unwrap_or(p).display().to_string()
}

#[test]
fn normalizing_keeps_a_function_union_member_distinct() {
    assert_ne!(
        normalize_type("(() => number) | string"),
        normalize_type("() => number | string")
    );
}

#[test]
fn normalizing_equates_equivalent_spellings() {
    assert_eq!(
        normalize_type("string | (() => number)"),
        normalize_type("(() => number) | string")
    );
    assert_eq!(
        normalize_type("(value: number) => string | null"),
        normalize_type("(arg0: number) => null | string")
    );
    assert_eq!(
        normalize_type("{ p: string; m(): number; }"),
        normalize_type("{ m: () => number; p: string }")
    );
    assert_eq!(
        normalize_type(r#""a\"b" | "c""#),
        normalize_type(r#""c" | "a\"b""#)
    );
    assert_eq!(
        normalize_type("[first: number, second: string]"),
        normalize_type("[number, string]")
    );
    assert_eq!(
        normalize_type("string | readonly (number | null)[]"),
        normalize_type("readonly (null | number)[] | string")
    );
}

#[test]
fn normalizing_index_signatures_ignores_only_parameter_names() {
    assert_eq!(
        normalize_type("{ readonly [name: string]: number | null; x: number; }"),
        normalize_type("{ x: number; readonly [key: string]: null | number }")
    );
    for distinct in [
        "{ [key: number]: number }",
        "{ readonly [key: string]: number }",
        "{ [key: string]: string }",
        "{ key: number }",
    ] {
        assert_ne!(
            normalize_type("{ [name: string]: number }"),
            normalize_type(distinct)
        );
    }
}

/// `(rule, tsc's text, ours)`, asserted equal after normalizing.
const TSC_ONLY_SPELLINGS: &[(&str, &str, &str)] = &[
    (
        "Record<string, V>",
        "Record<string, number | null>",
        "{ [key: string]: null | number }",
    ),
    ("ArrayIterator", "ArrayIterator<number>", "Iterator<number>"),
    (
        "optional undefined",
        "{ sn?: string | number | undefined; }",
        "{ sn?: number | string }",
    ),
    (
        "b?: undefined",
        "{ a: number; b?: undefined; } | { a: number; b: string; }",
        "{ a: number } | { a: number; b: string }",
    ),
    (
        "binding pattern",
        "([a, { b }, ...c]: number[]) => void",
        "(arg0: number[]) => void",
    ),
    (
        "literal beside its base",
        r#"string | "bar" | number | 1 | -2 | boolean | true | bigint | 1n"#,
        "bigint | boolean | number | string",
    ),
];

/// `(rule, tsc's text, ours)`, which must stay different after normalizing.
const DISTINCT_SPELLINGS: &[(&str, &str, &str)] = &[
    (
        "Record with a number key",
        "Record<number, string>",
        "{ [key: number]: string }",
    ),
    (
        "Record with three arguments",
        "Record<string, string, number>",
        "{ [key: string]: string }",
    ),
    (
        "optional null",
        "{ sn?: number | string }",
        "{ sn?: null | number | string }",
    ),
    (
        "required field",
        "{ sn: number | string }",
        "{ sn?: number | string }",
    ),
    (
        "our null for tsc's undefined field",
        "{ b?: undefined }",
        "{ b?: null }",
    ),
    ("undefined array", "{ a?: undefined[] }", "{}"),
    ("a bigint literal isn't a number", "number | 1n", "number"),
    (
        "a string literal isn't a number",
        r#"number | "1""#,
        "number",
    ),
    (
        "a string literal isn't a boolean",
        r#"boolean | "true""#,
        "boolean",
    ),
    ("a literal without its base", r#""bar" | number"#, "number"),
];

#[test]
fn normalizing_reads_tsc_only_spellings_as_ours() {
    for (rule, tsc, ours) in TSC_ONLY_SPELLINGS {
        assert_eq!(normalize_type(tsc), normalize_type(ours), "{rule}");
    }
}

#[test]
fn normalizing_keeps_distinct_types_distinct() {
    for (rule, tsc, ours) in DISTINCT_SPELLINGS {
        assert_ne!(normalize_type(tsc), normalize_type(ours), "{rule}");
    }
}

#[test]
fn only_this_as_this_counts_as_its_class() {
    assert!(same_type("this", "this", "Base2<T>"));
    assert!(same_type("this", "this", "Derived"));
    assert!(!same_type("x", "this", "Base2<T>"));
    for not_a_class in [
        "<error>",
        "unknown",
        "number",
        "A | B",
        "{ x: number }",
        "() => void",
    ] {
        assert!(!same_type("this", "this", not_a_class), "{not_a_class}");
    }
}

#[test]
fn normalizing_keeps_readonly_distinct() {
    assert_ne!(
        normalize_type("readonly number[]"),
        normalize_type("number[]")
    );
    assert_ne!(
        normalize_type("readonly number[][]"),
        normalize_type("(readonly number[])[]")
    );
}

#[test]
fn normalizing_drops_only_the_buffer_of_a_uint8array() {
    for buffer in ["ArrayBuffer", "ArrayBufferLike"] {
        assert_eq!(
            normalize_type(&format!("string | Uint8Array<{buffer}>")),
            normalize_type("Uint8Array | string")
        );
    }
    assert_ne!(
        normalize_type("Map<string, ArrayBuffer>"),
        normalize_type("Map")
    );
}

#[test]
fn only_a_single_literal_is_skipped() {
    for literal in [
        "+1",
        "1e-5",
        "-1E+5",
        "- 10000000000000",
        "1",
        "-1",
        "1.5",
        ".5",
        "0x10",
        "10n",
        "'a'",
        r#""a\"b""#,
        "`t`",
        "true",
        "null",
    ] {
        assert!(is_literal(literal), "{literal}");
    }
    for expression in [
        "_",
        "_1",
        "-_1",
        "_e1",
        "0xe-1",
        "1.5.toFixed",
        "1e3.toFixed",
        "1e",
        ".",
        r#""a" + "b""#,
        "'a' == 'b'",
        "`a${b}`",
        "NaN",
        "Infinity",
        "x",
        "- x",
        "-(1)",
        "- 1 + 2",
    ] {
        assert!(!is_literal(expression), "{expression}");
    }
}

#[test]
fn an_entry_splits_at_its_caret_column_in_utf16_units() {
    // The writer pads the caret line with one space per UTF-16 unit of the text.
    let caret = |text: &str, ty: &str| {
        format!(
            "{} : {}",
            " ".repeat(text.encode_utf16().count()),
            "^".repeat(ty.len())
        )
    };
    let text = r#"len("héllo")"#;
    let entry = split_entry(&format!("{text} : number"), &caret(text, "number")).expect("entry");
    assert_eq!((entry.text.as_str(), entry.ty.as_str()), (text, "number"));

    let ternary = split_entry("a ? b : c : string", &caret("a ? b : c", "string")).expect("entry");
    assert_eq!(ternary.text, "a ? b : c");
}

#[test]
fn caret_lines_are_told_from_echoed_source() {
    assert!(is_caret_line(">          : ^^^^^^"));
    assert!(!is_caret_line(">a ? b : c : string"));
    assert!(!is_caret_line("> 0;"), "a source line that starts with `>`");
}

#[test]
fn option_lines_match_the_baseline_writer() {
    for line in [
        "// @strict: true",
        "//@target: es5",
        "  // @x : y",
        "\u{feff}// @strict: true",
    ] {
        assert!(is_option_line(line), "{line:?}");
    }
    for line in [
        "// @ts-ignore: reason",
        "// @a.b: 1",
        "// strict: true",
        "// @: x",
    ] {
        assert!(!is_option_line(line), "{line:?}");
    }
}

#[test]
fn only_a_supported_error_agrees_with_a_tsc_error_on_its_line() {
    let tsc = || {
        vec![BaselineError {
            line: 3,
            code: "TS2362".into(),
            message: "The left-hand side of an arithmetic operation must be ...".into(),
        }]
    };
    let ours = |unsupported: bool| {
        BTreeMap::from([(
            3,
            LineError {
                message: "any message".into(),
                unsupported,
            },
        )])
    };

    let (missed, extra) = compare_errors(tsc(), &ours(true), &agreed_lines(&tsc(), &ours(true)));
    assert_eq!((missed.len(), extra.len()), (1, 1));
    assert!(extra[0].tsc_rejects);

    let (missed, extra) = compare_errors(tsc(), &ours(false), &agreed_lines(&tsc(), &ours(false)));
    assert_eq!((missed.len(), extra.len()), (0, 0));
}

#[test]
fn syntax_and_named_gaps_lack_support_and_other_errors_do_not() {
    let source = "let a: number = \"s\";\nlet b = 1 +;\nlet c: any = 1;\n\
                  type D<T, T> = T;\nfunction main(): void {}\n";
    let (_, errors) = case_errors::case_errors(source);
    let messages: Vec<&str> = errors.iter().map(|e| e.message.as_str()).collect();
    let on = |line: usize| {
        errors
            .iter()
            .find(|e| e.line == line)
            .map(|e| e.support == case_errors::Support::Lacking)
    };
    assert_eq!(on(1), Some(false), "a type mismatch: {messages:?}");
    assert_eq!(on(2), Some(true), "a parse error: {messages:?}");
    assert_eq!(on(3), Some(true), "`any`: {messages:?}");
    assert_eq!(on(4), Some(false), "a check `tsc` makes too: {messages:?}");
}

#[test]
fn entries_prefer_retained_loop_reads_and_keep_erased_source_nodes() {
    let cases = [
        ("controlFlow/controlFlowNoIntermediateErrors", 17),
        ("types/stringLiteral/stringLiteralTypesOverloads04", 8),
    ];
    for (case, compared) in cases {
        let path = Path::new(ROOT)
            .join("typescript")
            .join(format!("{case}.ts"));
        let report = compare_case(&path).expect("conformance case");
        assert_eq!(
            report.compared.len(),
            compared,
            "source coverage for {case}"
        );
        assert!(report.type_divergences.is_empty(), "{}", report.render());
    }
}
