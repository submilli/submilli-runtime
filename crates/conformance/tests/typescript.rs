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

#[path = "support/case_errors.rs"]
mod case_errors;
#[path = "support/typed_reachability.rs"]
mod typed_reachability;

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use interpreter::{ExprId, Span, StmtId, Type, TypedAst, TypedExprKind, TypedStmtKind};

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

/// The files that belong to a case, beside its `.ts`.
const CASE_FILE_SUFFIXES: &[&str] = &[".types", ".errors.txt", ".divergences"];

#[test]
fn typescript_baselines() {
    let root = Path::new(ROOT).join("typescript");
    let update = std::env::var("UPDATE_TYPESCRIPT_EXPECTED").is_ok_and(|v| v != "0");
    let mut failures = orphaned_case_files(&root, update);

    let cases = find_cases(&root);
    let mut totals = Totals::default();
    for case in &cases {
        match compare_case(case) {
            Ok(report) => {
                totals.add(&report);
                failures.extend(check_divergences(case, &report, update));
            }
            Err(message) => failures.push(format!("--- {} ---\n{message}", rel(case))),
        }
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

/// Baselines and `.divergences` left behind by a case that no longer exists.
/// They would otherwise go unnoticed, since cases are found by their `.ts`.
/// Update mode removes them.
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
        if update {
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

/// The `.ts` a baseline or `.divergences` file belongs to.
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
        self.compared += report.compared;
        self.type_divergences += report.type_divergences.len();
        self.missed_errors += report.missed_errors.len();
        self.extra_errors += report.extra_errors.len();
    }
}

/// Everything one case disagrees with `tsc` about.
struct Report {
    baseline_entries: usize,
    compared: usize,
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
            self.compared,
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
    let (missed_errors, extra_errors) = compare_errors(tsc_errors, &our_errors, &agreed_lines);
    Ok(Report {
        baseline_entries: baseline.values().map(Vec::len).sum(),
        compared,
        type_divergences,
        missed_errors,
        extra_errors,
    })
}

/// How many `tsc` entries were compared, and those whose type differs from ours.
///
/// Skips a pair where a side's type is its error recovery, which says nothing about
/// inference: ours holding `<error>` on a line where we report an error (the error
/// is recorded already), and `tsc`'s `any` on a line where the two errors agree.
fn compare_types(
    baseline: &EntriesByLine,
    ours: &EntriesByLine,
    our_error_lines: &BTreeSet<usize>,
    agreed_lines: &BTreeSet<usize>,
) -> (usize, Vec<TypeDivergence>) {
    let error_type = Type::Error.to_string();
    let is_recovery = |line: &usize, tsc_ty: &str, our_ty: &str| {
        (our_ty.contains(&error_type) && our_error_lines.contains(line))
            || (tsc_ty == "any" && agreed_lines.contains(line))
    };
    let mut compared = 0;
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
                compared += 1;
                if normalize_type(tsc_ty) != normalize_type(our_ty) {
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
        let e = typed.expr(id);
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
        match &typed.stmt(id).kind {
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
        || is_number_literal(text.strip_prefix(['-', '+']).unwrap_or(text))
        || is_string_literal(text)
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

/// A type's text in a canonical form, so that two spellings of the same type
/// compare equal: union members and object fields sorted, parameter names
/// dropped (`tsc` prints the declared name, we print `arg0`), method signatures
/// read as function-typed fields, and no `;` before an object's `}`.
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

impl TypeText<'_> {
    fn union(&mut self) -> Option<CanonicalType> {
        let mut members = vec![self.postfix()?];
        while self.eat(" | ") {
            members.push(self.postfix()?);
        }
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
            return Some(CanonicalType::plain(format!("{name}<{}>", args.join(", "))));
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
        self.word()?;
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
            let ty = if self.rest().starts_with('(') {
                // A method signature, `m(a: T): R`, is the field `m: (a: T) => R`.
                self.eat("(");
                let params = self.list(")", Self::parameter)?;
                if !self.eat(": ") {
                    return None;
                }
                format!("({}) => {}", params.join(", "), self.union_text()?)
            } else if self.eat(": ") {
                self.union_text()?
            } else {
                return None;
            };
            fields.push(format!("{readonly}{name}{optional}: {ty}"));
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
fn only_a_single_literal_is_skipped() {
    for literal in [
        "+1",
        "1e-5",
        "-1E+5",
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
        assert_eq!(report.compared, compared, "source coverage for {case}");
        assert!(report.type_divergences.is_empty(), "{}", report.render());
    }
}
