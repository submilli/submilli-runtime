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

use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use interpreter::{
    Diagnostic, ExprId, FileId, Severity, Span, StmtId, Type, TypedAst, TypedExprKind,
    TypedStmtKind, typecheck_to_typed_ast,
};

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

/// The files that belong to a case, beside its `.ts`.
const CASE_FILE_SUFFIXES: &[&str] = &[".types", ".errors.txt", ".divergences"];

#[test]
fn typescript_baselines() {
    let root = Path::new(ROOT).join("typescript");
    let update = std::env::var("UPDATE_TYPESCRIPT_EXPECTED").is_ok_and(|v| v != "0");
    let mut failures = orphaned_case_files(&root, update);

    let mut cases = Vec::new();
    collect_files(
        &root,
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

    let mut totals = Totals::default();
    for case in &cases {
        let report = match compare_case(case) {
            Ok(report) => report,
            Err(message) => {
                failures.push(format!("--- {} ---\n{message}", rel(case)));
                continue;
            }
        };
        totals.add(&report);
        let rendered = report.render();
        let expected_path = case.with_extension("divergences");
        let expected = fs::read_to_string(&expected_path).unwrap_or_default();
        if rendered == expected {
            continue;
        }
        if update {
            fs::write(&expected_path, &rendered)
                .unwrap_or_else(|e| panic!("write {}: {e}", expected_path.display()));
        } else {
            failures.push(format!(
                "--- {} ---\ndiverges differently from its committed file \
                 (rerun with UPDATE_TYPESCRIPT_EXPECTED=1 if the change is intended)\n\
                 expected:\n{expected}\nactual:\n{rendered}",
                rel(&expected_path)
            ));
        }
    }

    eprintln!(
        "typescript: {} case(s); {} of {} tsc types compared, {} differ; \
         {} tsc error line(s) we accept, {} error line(s) tsc accepts",
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
    /// Lines where `tsc` reports an error and we report none.
    missed_errors: Vec<BaselineError>,
    /// Our errors on lines where `tsc` reports none.
    extra_errors: Vec<(usize, String)>,
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
        for e in &self.missed_errors {
            out.push_str(&format!(
                "line {}: tsc {}: {} (we accept)\n",
                e.line, e.code, e.message
            ));
        }
        for (line, message) in &self.extra_errors {
            out.push_str(&format!("line {line}: ours: {message} (tsc accepts)\n"));
        }
        out
    }
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
    let (typed, diags) = typecheck_to_typed_ast(&source, FileId(0));
    let ours = entries_by_line(&typed, &source);
    let (compared, type_divergences) = compare_types(&baseline, &ours);
    let (missed_errors, extra_errors) =
        compare_errors(read_baseline_errors(case), error_lines(&diags, &source));
    Ok(Report {
        baseline_entries: baseline.values().map(Vec::len).sum(),
        compared,
        type_divergences,
        missed_errors,
        extra_errors,
    })
}

/// How many `tsc` entries were compared, and those whose type differs from ours.
fn compare_types(baseline: &EntriesByLine, ours: &EntriesByLine) -> (usize, Vec<TypeDivergence>) {
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

/// `tsc`'s errors on lines where we report none, and ours on lines where it
/// reports none.
fn compare_errors(
    tsc_errors: Vec<BaselineError>,
    our_errors: BTreeMap<usize, String>,
) -> (Vec<BaselineError>, Vec<(usize, String)>) {
    let extra = our_errors
        .iter()
        .filter(|(line, _)| !tsc_errors.iter().any(|e| e.line == **line))
        .map(|(line, message)| (*line, message.clone()))
        .collect();
    let missed = tsc_errors
        .into_iter()
        .filter(|e| !our_errors.contains_key(&e.line))
        .collect();
    (missed, extra)
}

/// Our type for every expression and every name a declaration or assignment
/// binds.
fn entries_by_line(typed: &TypedAst, source: &str) -> EntriesByLine {
    let mut nodes: Vec<(Span, Claim, Type)> = (0..typed.exprs_len())
        .map(|i| {
            let e = typed.expr(ExprId(i as u32));
            (
                e.span,
                Claim::of(&e.kind, i, span_text(source, e.span)),
                e.ty.clone(),
            )
        })
        .collect();
    nodes.extend(
        bindings(typed)
            .into_iter()
            .map(|(span, ty)| (span, Claim::Binding, ty)),
    );
    nodes.retain(|(span, _, _)| span_text(source, *span).is_some());
    nodes.sort_by(|(a, a_claim, _), (b, b_claim, _)| {
        (a.start, Reverse(a.end), Reverse(a_claim)).cmp(&(
            b.start,
            Reverse(b.end),
            Reverse(b_claim),
        ))
    });
    nodes.dedup_by(|a, b| a.0.start == b.0.start && a.0.end == b.0.end);

    let mut by_line = EntriesByLine::new();
    for (span, _, ty) in nodes {
        by_line
            .entry(line_of(source, span.start as usize))
            .or_default()
            .push(Entry {
                text: collapse_whitespace(span_text(source, span).unwrap_or_default()),
                ty: ty.to_string(),
            });
    }
    by_line
}

/// Which of the nodes sharing a span is the one the source wrote; the greatest
/// wins. The typechecker gives each node it synthesizes (a default argument, a
/// rest parameter's array, the comparisons a `switch` expands to, a narrowed or
/// captured read) the span of the construct it belongs to.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Claim {
    /// A variable reference whose span is not a name: a closure's capture.
    SynthesizedReference,
    /// Any other expression. The last one created at a span is the construct
    /// itself, created after the pieces synthesized for it.
    Expression(usize),
    /// A variable reference at a name. The first one created is the read the
    /// source wrote; a narrowed copy comes after it.
    Reference(Reverse<usize>),
    /// A declared or assigned name, typed by what the binding holds.
    Binding,
}

impl Claim {
    fn of(kind: &TypedExprKind, index: usize, text: Option<&str>) -> Self {
        let is_reference = matches!(
            kind,
            TypedExprKind::LocalRef { .. }
                | TypedExprKind::LocalNarrowRef { .. }
                | TypedExprKind::GlobalRef { .. }
                | TypedExprKind::FunctionRef { .. }
        );
        match (is_reference, text.is_some_and(is_identifier)) {
            (true, true) => Claim::Reference(Reverse(index)),
            (true, false) => Claim::SynthesizedReference,
            (false, _) => Claim::Expression(index),
        }
    }
}

/// Each name a declaration or assignment binds, with the binding's type. `tsc`
/// types an assignment's target, like a declaration's name, by what the binding
/// holds, not by the value being assigned.
fn bindings(typed: &TypedAst) -> Vec<(Span, Type)> {
    let mut out: Vec<(Span, Type)> = typed
        .globals
        .iter()
        .map(|g| (g.name.span, g.ty.clone()))
        .collect();
    for function in &typed.functions {
        out.extend(function.params.iter().map(|p| (p.name.span, p.ty.clone())));
    }
    for i in 0..typed.stmts_len() {
        match &typed.stmt(StmtId(i as u32)).kind {
            TypedStmtKind::Let { name, ty, .. } | TypedStmtKind::Const { name, ty, .. } => {
                out.push((name.span, ty.clone()));
            }
            TypedStmtKind::ForOf {
                name, element_ty, ..
            } => out.push((name.span, element_ty.clone())),
            TypedStmtKind::AssignLocal {
                ident, target_ty, ..
            }
            | TypedStmtKind::AssignGlobal {
                ident, target_ty, ..
            } => out.push((ident.span, target_ty.clone())),
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
    for (i, line) in lines.iter().enumerate().skip(body_start) {
        let Some(entry) = line.strip_prefix('>') else {
            if !line.trim().is_empty() {
                source_index = Some(source_index.map_or(0, |n| n + 1));
            }
            continue;
        };
        let Some(caret_line) = lines.get(i + 1).and_then(|l| l.strip_prefix('>')) else {
            continue;
        };
        let Some(parsed) = split_entry(entry, caret_line) else {
            continue;
        };
        let Some(&line_no) = source_index.and_then(|n| case_lines.get(n)) else {
            continue;
        };
        entries.entry(line_no).or_default().push(parsed);
    }
    Ok(entries)
}

/// The 1-based numbers of the case's lines a baseline echoes: the non-blank lines
/// that are not `// @option:` lines.
fn countable_line_numbers(source: &str) -> Vec<usize> {
    source
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty() && !is_option_line(l))
        .map(|(i, _)| i + 1)
        .collect()
}

/// Splits a `>text : type` entry at the column of the ` : ` in the caret line
/// under it — the only reliable split, since the text can itself contain ` : `.
/// That column counts UTF-16 units of the text, as `tsc` measures strings. `None`
/// for the caret line itself, whose text is blank.
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
    let text = &entry[..colon];
    if text.trim().is_empty() {
        return None;
    }
    Some(Entry {
        text: collapse_whitespace(text),
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

/// The first of our errors on each line.
fn error_lines(diags: &[Diagnostic], source: &str) -> BTreeMap<usize, String> {
    let mut lines = BTreeMap::new();
    for d in diags
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
    {
        lines
            .entry(line_of(source, d.span.start as usize))
            .or_insert_with(|| d.message.clone());
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
        || is_number_literal(text.strip_prefix('-').unwrap_or(text))
        || is_string_literal(text)
}

/// `1`, `1.5`, `.5`, `1e3`, `0x10`, `1_000`, `10n`. Not `NaN` or `Infinity`, which
/// are names.
fn is_number_literal(text: &str) -> bool {
    let digits = text.strip_prefix('.').unwrap_or(text);
    digits.starts_with(|c: char| c.is_ascii_digit())
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_'))
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

/// A type read by `TypeText`. A function type is marked, because as a union
/// member it needs parentheses: `(() => A) | B` is not `() => A | B`.
struct Read {
    text: String,
    is_function: bool,
}

impl Read {
    fn plain(text: String) -> Self {
        Read {
            text,
            is_function: false,
        }
    }
}

impl TypeText<'_> {
    fn union(&mut self) -> Option<Read> {
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
        Some(Read::plain(texts.join(" | ")))
    }

    fn union_text(&mut self) -> Option<String> {
        self.union().map(|read| read.text)
    }

    fn postfix(&mut self) -> Option<Read> {
        let mut ty = self.primary()?;
        while self.eat("[]") {
            ty = Read::plain(format!("({})[]", ty.text));
        }
        Some(ty)
    }

    fn primary(&mut self) -> Option<Read> {
        if self.rest().starts_with('(') {
            return self.function_or_group();
        }
        if self.eat("{") {
            return self.object().map(Read::plain);
        }
        if self.eat("[") {
            let elements = self.list("]", Self::union_text)?;
            return Some(Read::plain(format!("[{}]", elements.join(", "))));
        }
        if self.rest().starts_with('"') {
            return self.string_literal().map(Read::plain);
        }
        let name = self.word()?;
        // The port spells `undefined` as `null`, so `tsc`'s `undefined` is ours.
        if name == "undefined" {
            return Some(Read::plain("null".to_string()));
        }
        if self.eat("<") {
            let args = self.list(">", Self::union_text)?;
            return Some(Read::plain(format!("{name}<{}>", args.join(", "))));
        }
        Some(Read::plain(name))
    }

    /// `(a: T, b?: U) => R` reads as `(T, U?) => R`; anything else in
    /// parentheses is a grouped type, read as what it groups.
    fn function_or_group(&mut self) -> Option<Read> {
        let start = self.pos;
        self.eat("(");
        if let Some(params) = self.list(")", Self::parameter)
            && self.eat(" => ")
        {
            let ret = self.union_text()?;
            return Some(Read {
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
            let name = self.word()?;
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
            fields.push(format!("{name}{optional}: {ty}"));
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

/// A `// @name: value` compiler-option line. Must agree with `OPTION_LINE` in
/// `typescript-baselines/write-baselines.cjs`, which leaves these lines out of the
/// baseline: `^\s*//\s*@\w+\s*:`. JavaScript's `\s` includes a byte-order mark.
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
}

#[test]
fn only_a_single_literal_is_skipped() {
    for literal in [
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

    let caret_line = caret("a ? b : c", "string");
    assert!(split_entry(&caret_line, &caret_line).is_none());
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
