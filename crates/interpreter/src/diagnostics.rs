use std::fmt::Write;

use crate::source::Sources;
use crate::{LineIndex, Span};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub span: Span,
    pub message: String,
    /// Unanchored hint blocks rendered after the primary, before notes. Span-anchored secondaries go in `notes`.
    pub help: Vec<String>,
    pub notes: Vec<(Span, String)>,
}

pub fn render(diagnostic: &Diagnostic, sources: &Sources) -> String {
    let gutter_width = gutter_width_for_spans(
        sources,
        std::iter::once(diagnostic.span).chain(diagnostic.notes.iter().map(|(s, _)| *s)),
    );
    let gutter_blank = " ".repeat(gutter_width);

    let mut out = String::new();
    writeln!(
        out,
        "{}: {}",
        severity_str(diagnostic.severity),
        diagnostic.message
    )
    .unwrap();
    render_anchored_block(
        &mut out,
        sources,
        diagnostic.span,
        &gutter_blank,
        gutter_width,
    );

    for help in &diagnostic.help {
        writeln!(out, "{gutter_blank} |").unwrap();
        writeln!(out, "help: {help}").unwrap();
    }

    for (note_span, note_msg) in &diagnostic.notes {
        writeln!(out, "{gutter_blank} |").unwrap();
        writeln!(out, "note: {note_msg}").unwrap();
        render_anchored_block(&mut out, sources, *note_span, &gutter_blank, gutter_width);
    }

    out
}

fn severity_str(s: Severity) -> &'static str {
    match s {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}

fn gutter_width_for_spans(sources: &Sources, spans: impl IntoIterator<Item = Span>) -> usize {
    let mut max_line: u32 = 1;
    for span in spans {
        let Some(file) = sources.get(span.file) else {
            continue;
        };
        let line_index = file.line_index();
        let line_count = line_index.line_count().max(1);
        let (end_line, _) = line_index.line_col(span.end);
        // +1 for the context line below, clamped to file bounds.
        max_line = max_line.max(end_line.saturating_add(1).min(line_count));
    }
    max_line.to_string().len()
}

fn render_anchored_block(
    out: &mut String,
    sources: &Sources,
    span: Span,
    gutter_blank: &str,
    gutter_width: usize,
) {
    let Some(file) = sources.get(span.file) else {
        // Reserved (prelude/stdlib) ids aren't in the registry; show their
        // virtual path. Anything else is a bug — fall back rather than panic.
        let path = span.file.reserved_path().unwrap_or("<unknown>");
        writeln!(out, "{gutter_blank}--> {path}").unwrap();
        return;
    };
    let line_index = file.line_index();
    let (start_line, start_col) = line_index.line_col(span.start);
    writeln!(
        out,
        "{gutter_blank}--> {}:{start_line}:{start_col}",
        file.path
    )
    .unwrap();
    writeln!(out, "{gutter_blank} |").unwrap();
    render_source_block(
        out,
        &file.text,
        line_index,
        span,
        gutter_blank,
        gutter_width,
    );
}

/// Source-context block for `span`. Shared with `backtrace` so compile and
/// runtime outputs use the same `N | text` / `  | ^^^` shape. Caller owns
/// the `--> file:line:col` header.
pub(crate) fn render_source_block(
    out: &mut String,
    source: &str,
    line_index: &LineIndex,
    span: Span,
    gutter_blank: &str,
    gutter_width: usize,
) {
    let (start_line, start_col) = line_index.line_col(span.start);
    let (end_line, end_col) = line_index.line_col(span.end);
    let line_count = line_index.line_count();

    if start_line > 1 {
        let prev = line_index.line_text(source, start_line - 1);
        writeln!(
            out,
            "{:>width$} | {}",
            start_line - 1,
            prev,
            width = gutter_width
        )
        .unwrap();
    }

    let start_text = line_index.line_text(source, start_line);
    writeln!(out, "{start_line:>gutter_width$} | {start_text}").unwrap();
    let caret_indent = chars_in_byte_range(start_text, 0, start_col - 1);
    let caret_len = if start_line == end_line {
        chars_in_byte_range(start_text, start_col - 1, end_col - 1).max(1)
    } else {
        // Multi-line: extend to end of line.
        chars_in_byte_range(start_text, start_col - 1, u32::MAX).max(1)
    };
    writeln!(
        out,
        "{} | {}{}",
        gutter_blank,
        " ".repeat(caret_indent),
        "^".repeat(caret_len)
    )
    .unwrap();

    if end_line > start_line {
        for mid_line in (start_line + 1)..end_line {
            let mid_text = line_index.line_text(source, mid_line);
            writeln!(out, "{mid_line:>gutter_width$} | {mid_text}").unwrap();
            let mid_carets = mid_text.chars().count().max(1);
            writeln!(out, "{} | {}", gutter_blank, "^".repeat(mid_carets)).unwrap();
        }
        let end_text = line_index.line_text(source, end_line);
        writeln!(out, "{end_line:>gutter_width$} | {end_text}").unwrap();
        let end_carets = chars_in_byte_range(end_text, 0, end_col.saturating_sub(1)).max(1);
        writeln!(out, "{} | {}", gutter_blank, "^".repeat(end_carets)).unwrap();
    }

    // Skip at EOF; line_count is the last addressable line.
    if end_line < line_count {
        let next = line_index.line_text(source, end_line + 1);
        writeln!(
            out,
            "{:>width$} | {}",
            end_line + 1,
            next,
            width = gutter_width
        )
        .unwrap();
    }
}

/// Characters in `line[from..to]`, where `from`/`to` are byte columns as
/// `LineIndex::line_col` reports them, clamped to the line. Carets are drawn one per
/// character, so a multi-byte character before or inside the span counts once. (A
/// double-width character such as CJK or emoji still takes two terminal columns.)
fn chars_in_byte_range(line: &str, from: u32, to: u32) -> usize {
    let clamp = |col: u32| {
        let mut at = (col as usize).min(line.len());
        while !line.is_char_boundary(at) {
            at -= 1;
        }
        at
    };
    let (from, to) = (clamp(from), clamp(to));
    line[from..to.max(from)].chars().count()
}

#[cfg(test)]
mod tests {
    use super::{Diagnostic, Severity, render};
    use crate::source::Sources;
    use crate::{FileId, Span};

    const F: FileId = FileId(0);

    fn sources(text: &str) -> Sources {
        let (sources, _) = Sources::single("script.subm", text);
        sources
    }

    #[test]
    fn construct_and_read_fields() {
        let d = Diagnostic {
            severity: Severity::Error,
            span: Span::new(F, 10, 20),
            message: "type mismatch".to_string(),
            help: vec![],
            notes: vec![(Span::new(F, 5, 8), "defined here".to_string())],
        };
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.span, Span::new(F, 10, 20));
        assert_eq!(d.message, "type mismatch");
        assert_eq!(d.notes.len(), 1);
        assert_eq!(d.notes[0].0, Span::new(F, 5, 8));
        assert_eq!(d.notes[0].1, "defined here");
    }

    #[test]
    fn clone_is_equal() {
        let d = Diagnostic {
            severity: Severity::Warning,
            span: Span::new(F, 0, 3),
            message: "unused variable".to_string(),
            help: vec![],
            notes: vec![],
        };
        assert_eq!(d.clone(), d);
    }

    #[test]
    fn render_single_line_error() {
        let source = "let x = 1;\nlet y: number = \"hello\";\nlet z = 2;\n";
        let diag = Diagnostic {
            severity: Severity::Error,
            span: Span::new(F, 27, 34),
            message: "type mismatch: expected `number`, found `string`".to_string(),
            help: vec![],
            notes: vec![],
        };
        insta::assert_snapshot!(render(&diag, &sources(source)));
    }

    #[test]
    fn carets_count_characters_not_bytes() {
        // `é` and `—` are 2 and 3 bytes; the carets must still sit under `bad`.
        let source = "let s = \"é—\"; bad;\n";
        let start = source.find("bad").unwrap() as u32;
        let diag = Diagnostic {
            severity: Severity::Error,
            span: Span::new(F, start, start + 3),
            message: "unresolved identifier `bad`".to_string(),
            help: vec![],
            notes: vec![],
        };
        let rendered = render(&diag, &sources(source));
        let caret_line = rendered.lines().find(|l| l.contains('^')).unwrap();
        let source_line = rendered.lines().find(|l| l.contains("bad;")).unwrap();
        let caret_col = caret_line.chars().position(|c| c == '^').unwrap();
        let bad_col = source_line[..source_line.find("bad").unwrap()]
            .chars()
            .count();
        assert_eq!(caret_col, bad_col, "{rendered}");
        assert_eq!(caret_line.matches('^').count(), 3, "{rendered}");
    }

    #[test]
    fn render_multi_note_error_with_gutter_alignment() {
        // 12 lines, each "N\n" padded so line 11 exists to force 2-digit gutter.
        let source = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\n";
        let diag = Diagnostic {
            severity: Severity::Error,
            span: Span::new(F, 2, 3), // "b" on line 2
            message: "missing return in all code paths".to_string(),
            help: vec![],
            notes: vec![
                (Span::new(F, 0, 1), "function declared here".to_string()),
                (
                    Span::new(F, 20, 21),
                    "this branch returns, but else does not".to_string(),
                ),
            ],
        };
        insta::assert_snapshot!(render(&diag, &sources(source)));
    }

    #[test]
    fn render_zero_length_span_at_eof() {
        let source = "hello";
        let diag = Diagnostic {
            severity: Severity::Error,
            span: Span::new(F, 5, 5),
            message: "expected `;`".to_string(),
            help: vec![],
            notes: vec![],
        };
        insta::assert_snapshot!(render(&diag, &sources(source)));
    }

    #[test]
    fn render_warning_severity() {
        let source = "let unused = 42;\n";
        let diag = Diagnostic {
            severity: Severity::Warning,
            span: Span::new(F, 4, 10),
            message: "unused variable `unused`".to_string(),
            help: vec![],
            notes: vec![],
        };
        insta::assert_snapshot!(render(&diag, &sources(source)));
    }

    #[test]
    fn render_clamps_at_sof() {
        // Primary on line 1: no line above; line below should still render.
        let source = "first\nsecond\nthird\n";
        let diag = Diagnostic {
            severity: Severity::Error,
            span: Span::new(F, 0, 5),
            message: "boom".to_string(),
            help: vec![],
            notes: vec![],
        };
        insta::assert_snapshot!(render(&diag, &sources(source)));
    }

    #[test]
    fn render_clamps_at_eof() {
        // Primary on the last content line of a file without trailing
        // newline: line above renders, no line below.
        let source = "first\nsecond\nthird";
        let diag = Diagnostic {
            severity: Severity::Error,
            span: Span::new(F, 13, 18),
            message: "boom".to_string(),
            help: vec![],
            notes: vec![],
        };
        insta::assert_snapshot!(render(&diag, &sources(source)));
    }

    #[test]
    fn render_multiline_span_carets() {
        // 3-line span starting mid-line 2 and ending mid-line 4: start
        // line carets extend to EOL, middle line is fully under
        // carets, end line stops at end column.
        let source = "line1\nline2 start\nmiddle\nend stop here\nline5\n";
        let start = source.find("start").unwrap() as u32;
        let end = source.find("end").unwrap() as u32 + 3;
        let diag = Diagnostic {
            severity: Severity::Error,
            span: Span::new(F, start, end),
            message: "multi-line span".to_string(),
            help: vec![],
            notes: vec![],
        };
        insta::assert_snapshot!(render(&diag, &sources(source)));
    }

    #[test]
    fn render_with_help() {
        let source = "let x = foo.bar;\n";
        let diag = Diagnostic {
            severity: Severity::Error,
            span: Span::new(F, 12, 15),
            message: "field `bar` does not exist on `Foo`".to_string(),
            help: vec!["interface Foo {\n  baz: number;\n}".to_string()],
            notes: vec![],
        };
        insta::assert_snapshot!(render(&diag, &sources(source)));
    }

    #[test]
    fn render_with_help_and_notes() {
        let source = "a\nb\nc\n";
        let diag = Diagnostic {
            severity: Severity::Error,
            span: Span::new(F, 2, 3),
            message: "bad".to_string(),
            help: vec!["try this instead".to_string()],
            notes: vec![(Span::new(F, 0, 1), "declared here".to_string())],
        };
        insta::assert_snapshot!(render(&diag, &sources(source)));
    }

    #[test]
    fn render_multiple_helps() {
        let source = "x\n";
        let diag = Diagnostic {
            severity: Severity::Error,
            span: Span::new(F, 0, 1),
            message: "bad".to_string(),
            help: vec!["first hint".to_string(), "second hint".to_string()],
            notes: vec![],
        };
        insta::assert_snapshot!(render(&diag, &sources(source)));
    }
}
