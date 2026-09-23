//! TypeScript conformance: compare what our typechecker infers against what
//! `tsc` recorded for the same program.
//!
//! Every case under `typescript/` is a test from TypeScript's own conformance
//! suite, ported with the mechanical rewrites in `typescript/README.md`, and sits
//! next to the two baselines TypeScript records for it:
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

#[test]
fn typescript_baselines() {
    let root = Path::new(ROOT).join("typescript");
    let mut cases = Vec::new();
    collect_cases(&root, &mut cases);
    cases.sort();
    if let Ok(filter) = std::env::var("CONFORMANCE_FILTER") {
        cases.retain(|p| p.to_string_lossy().contains(&filter));
    }
    assert!(!cases.is_empty(), "no cases under {}", root.display());

    let update = std::env::var("UPDATE_TYPESCRIPT_EXPECTED").is_ok_and(|v| v != "0");
    let mut stale = Vec::new();
    let mut totals = Totals::default();
    for case in &cases {
        let report = compare_case(case);
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
            stale.push(format!(
                "--- {} ---\nexpected:\n{expected}\nactual:\n{rendered}",
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
        stale.is_empty(),
        "\n{} case(s) diverge differently from their committed `.divergences` \
         (rerun with UPDATE_TYPESCRIPT_EXPECTED=1 if the change is intended):\n\n{}",
        stale.len(),
        stale.join("\n\n"),
    );
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

struct TypeDivergence {
    line: usize,
    text: String,
    tsc: String,
    ours: String,
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

fn compare_case(case: &Path) -> Report {
    let source = fs::read_to_string(case).unwrap_or_else(|e| panic!("read {}: {e}", rel(case)));
    let (typed, diags) = typecheck_to_typed_ast(&source, FileId(0));
    let ours = expressions_by_line(&typed, &source);
    let baseline = read_baseline_types(&case.with_extension("types"), &source);

    let mut compared = 0;
    let mut type_divergences = Vec::new();
    for (line, entries) in &baseline {
        let empty = Vec::new();
        let ours_on_line = ours.get(line).unwrap_or(&empty);
        for (text, tsc_types) in group_by_text(entries) {
            if is_literal(&text) {
                continue;
            }
            let our_types: Vec<&String> = ours_on_line
                .iter()
                .filter(|(t, _)| *t == text)
                .map(|(_, ty)| ty)
                .collect();
            // Pair occurrences in order only when both sides saw the same number of
            // them; otherwise a declaration name or a rewritten token would shift
            // every pairing after it.
            if our_types.len() != tsc_types.len() {
                continue;
            }
            for (tsc, ours) in tsc_types.iter().zip(our_types) {
                compared += 1;
                if normalize_type(tsc) != normalize_type(ours) {
                    type_divergences.push(TypeDivergence {
                        line: *line,
                        text: text.clone(),
                        tsc: tsc.to_string(),
                        ours: ours.clone(),
                    });
                }
            }
        }
    }

    let tsc_errors = read_baseline_errors(case);
    let our_errors = error_lines(&diags, &source);
    let missed_errors = tsc_errors
        .iter()
        .filter(|e| !our_errors.contains_key(&e.line))
        .cloned()
        .collect();
    let extra_errors = our_errors
        .into_iter()
        .filter(|(line, _)| !tsc_errors.iter().any(|e| e.line == *line))
        .collect();

    Report {
        baseline_entries: baseline.values().map(Vec::len).sum(),
        compared,
        type_divergences,
        missed_errors,
        extra_errors,
    }
}

/// Our type for every expression and every name a declaration or assignment
/// binds, keyed by the line it starts on, in source order, as
/// (whitespace-collapsed source text, rendered type).
fn expressions_by_line(typed: &TypedAst, source: &str) -> BTreeMap<usize, Vec<(String, String)>> {
    // Of the nodes sharing a span, keep the one the source wrote. The typechecker
    // gives the nodes it synthesizes (a default argument, a rest parameter's
    // array, the comparisons a `switch` expands to, a narrowed read) the span of
    // the construct they belong to. A binding's declared type wins outright. A
    // variable reference is the first reference node created at its span, before
    // any narrowed copy; any other expression is the last node created there,
    // after the pieces synthesized for it.
    let mut spans: Vec<(Span, (u8, i64), Type)> = (0..typed.exprs_len())
        .map(|i| {
            let e = typed.expr(ExprId(i as u32));
            let preference = if is_reference(&e.kind) {
                (1, -(i as i64))
            } else {
                (0, i as i64)
            };
            (e.span, preference, e.ty.clone())
        })
        .collect();
    spans.extend(
        bindings(typed)
            .into_iter()
            .map(|(span, ty)| (span, (2, 0), ty)),
    );
    spans.retain(|(span, _, _)| span.start < span.end && span.end as usize <= source.len());
    spans
        .sort_by_key(|(span, preference, _)| (span.start, Reverse(span.end), Reverse(*preference)));
    spans.dedup_by(|a, b| a.0.start == b.0.start && a.0.end == b.0.end);

    let mut by_line: BTreeMap<usize, Vec<(String, String)>> = BTreeMap::new();
    for (span, _, ty) in spans {
        let (start, end) = (span.start as usize, span.end as usize);
        by_line
            .entry(line_of(source, start))
            .or_default()
            .push((collapse_whitespace(&source[start..end]), ty.to_string()));
    }
    by_line
}

fn is_reference(kind: &TypedExprKind) -> bool {
    matches!(
        kind,
        TypedExprKind::LocalRef { .. }
            | TypedExprKind::LocalNarrowRef { .. }
            | TypedExprKind::GlobalRef { .. }
            | TypedExprKind::FunctionRef { .. }
    )
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

/// `tsc`'s entries from a `.types` baseline, keyed by line in the ported case.
///
/// The baseline echoes the upstream file without its `// @option:` lines, each
/// source line followed by `>text : type` entries for the expressions starting on
/// it. The port rewrites lines in place and keeps the option lines, so the n-th
/// non-blank, non-option line matches in both.
fn read_baseline_types(path: &Path, source: &str) -> BTreeMap<usize, Vec<(String, String)>> {
    let mut entries: BTreeMap<usize, Vec<(String, String)>> = BTreeMap::new();
    let Ok(baseline) = fs::read_to_string(path) else {
        return entries;
    };
    let case_lines: Vec<usize> = source
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty() && !is_option_line(l))
        .map(|(i, _)| i + 1)
        .collect();

    let lines: Vec<&str> = baseline.lines().map(|l| l.trim_end_matches('\r')).collect();
    let body_start = lines
        .iter()
        .position(|l| l.starts_with("=== "))
        .map_or(lines.len(), |i| i + 1);
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
        // The line under each entry puts its `:` in the same column, which is the
        // only reliable split: the expression text can itself contain ` : `.
        let Some(colon) = caret_line.find(" : ") else {
            continue;
        };
        if entry.get(..colon).is_none_or(|t| t.trim().is_empty()) {
            continue; // this is the caret line itself
        }
        let (Some(index), Some(ty)) = (source_index, entry.get(colon + 3..)) else {
            continue;
        };
        let Some(&line_no) = case_lines.get(index) else {
            continue;
        };
        entries
            .entry(line_no)
            .or_default()
            .push((collapse_whitespace(&entry[..colon]), ty.to_string()));
    }
    entries
}

#[derive(Clone)]
struct BaselineError {
    line: usize,
    code: String,
    message: String,
}

/// `tsc`'s errors in this case's own file, from the `file(line,col): error TSn: …`
/// summary at the top of `.errors.txt`.
fn read_baseline_errors(case: &Path) -> Vec<BaselineError> {
    let Ok(text) = fs::read_to_string(case.with_extension("errors.txt")) else {
        return Vec::new();
    };
    let file_name = case
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let mut errors: Vec<BaselineError> = Vec::new();
    for line in text.lines().map(|l| l.trim_end_matches('\r')) {
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

/// A literal written in the source. `tsc` gives each one its own literal type
/// (`1`, `"a"`, `true`) and widens it where it is bound; we widen at the literal
/// itself. Only the bound type is observable, and that is compared at the
/// binding, so the literal's own entry is skipped.
fn is_literal(text: &str) -> bool {
    let quoted = |q: char| text.len() >= 2 && text.starts_with(q) && text.ends_with(q);
    matches!(text, "true" | "false" | "null")
        || text.parse::<f64>().is_ok()
        || quoted('"')
        || quoted('\'')
        || (quoted('`') && !text.contains("${"))
}

fn group_by_text(entries: &[(String, String)]) -> Vec<(String, Vec<&str>)> {
    let mut groups: Vec<(String, Vec<&str>)> = Vec::new();
    for (text, ty) in entries {
        match groups.iter_mut().find(|(t, _)| t == text) {
            Some((_, types)) => types.push(ty),
            None => groups.push((text.clone(), vec![ty])),
        }
    }
    groups
}

/// A type's text in a canonical form, so that two spellings of the same type
/// compare equal: union members and object fields sorted, parameter names
/// dropped (`tsc` prints the declared name, we print `arg0`), method signatures
/// read as function-typed fields, and no `;` before an object's `}`. Text the reader does not understand is compared as written.
fn normalize_type(ty: &str) -> String {
    let text = collapse_whitespace(ty);
    let mut reader = TypeText {
        text: &text,
        pos: 0,
    };
    match reader.union() {
        Some(canonical) if reader.pos == text.len() => canonical,
        _ => text,
    }
}

/// A recursive-descent reader over the type syntax both printers produce. Each
/// method returns the canonical text of what it read.
struct TypeText<'a> {
    text: &'a str,
    pos: usize,
}

impl TypeText<'_> {
    fn union(&mut self) -> Option<String> {
        let mut members = vec![self.postfix()?];
        while self.eat(" | ") {
            members.push(self.postfix()?);
        }
        members.sort();
        members.dedup();
        Some(members.join(" | "))
    }

    fn postfix(&mut self) -> Option<String> {
        let mut ty = self.primary()?;
        while self.eat("[]") {
            ty = format!("({ty})[]");
        }
        Some(ty)
    }

    fn primary(&mut self) -> Option<String> {
        if self.rest().starts_with('(') {
            return self.function_or_group();
        }
        if self.eat("{") {
            return self.object();
        }
        if self.eat("[") {
            let elements = self.list("]", Self::union)?;
            return Some(format!("[{}]", elements.join(", ")));
        }
        if self.rest().starts_with('"') {
            return self.string_literal();
        }
        let name = self.word()?;
        // The port spells `undefined` as `null`, so `tsc`'s `undefined` is ours.
        if name == "undefined" {
            return Some("null".to_string());
        }
        if self.eat("<") {
            let args = self.list(">", Self::union)?;
            return Some(format!("{name}<{}>", args.join(", ")));
        }
        Some(name)
    }

    /// `(a: T, b?: U) => R` reads as `(T, U?) => R`; anything else in
    /// parentheses is a grouped type.
    fn function_or_group(&mut self) -> Option<String> {
        let start = self.pos;
        self.eat("(");
        if let Some(params) = self.list(")", Self::parameter)
            && self.eat(" => ")
        {
            let ret = self.union()?;
            return Some(format!("({}) => {ret}", params.join(", ")));
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
        Some(format!("{rest}{}{optional}", self.union()?))
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
                format!("({}) => {}", params.join(", "), self.union()?)
            } else if self.eat(": ") {
                self.union()?
            } else {
                return None;
            };
            fields.push(format!("{name}{optional}: {ty}"));
            let separated = self.eat(";") | self.eat(",");
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
        let end = rest[1..].find('"')? + 2;
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

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn line_of(source: &str, offset: usize) -> usize {
    source[..offset.min(source.len())].matches('\n').count() + 1
}

fn is_option_line(line: &str) -> bool {
    line.trim_start()
        .strip_prefix("//")
        .map(str::trim_start)
        .and_then(|rest| rest.strip_prefix('@'))
        .is_some_and(|rest| {
            rest.split_once(':')
                .is_some_and(|(name, _)| !name.contains(' '))
        })
}

fn collect_cases(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir({}): {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            collect_cases(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "ts") {
            out.push(path);
        }
    }
}

fn rel(p: &Path) -> String {
    p.strip_prefix(ROOT).unwrap_or(p).display().to_string()
}
