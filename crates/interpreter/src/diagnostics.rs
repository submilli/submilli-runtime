use crate::source::{SourceError, Sources};
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
    out.push_str(&format!(
        "{}: {}\n",
        severity_str(diagnostic.severity),
        diagnostic.message
    ));
    render_anchored_block(
        &mut out,
        sources,
        diagnostic.span,
        &gutter_blank,
        gutter_width,
    );

    for help in &diagnostic.help {
        out.push_str(&format!("{gutter_blank} |\n"));
        out.push_str(&format!("help: {help}\n"));
    }

    for (note_span, note_msg) in &diagnostic.notes {
        out.push_str(&format!("{gutter_blank} |\n"));
        out.push_str(&format!("note: {note_msg}\n"));
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
        let Ok((end_line, _)) = line_index.line_col(span.end) else {
            continue;
        };
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
    match source_context(sources, span, gutter_blank, gutter_width) {
        Ok(context) => out.push_str(&context),
        Err(error) => out.push_str(&format!(
            "{gutter_blank} | source context unavailable: {error}\n"
        )),
    }
}

fn source_context(
    sources: &Sources,
    span: Span,
    gutter_blank: &str,
    gutter_width: usize,
) -> Result<String, SourceError> {
    Span::new(span.file, span.start, span.end)?;
    if let Some(path) = span.file.reserved_path() {
        return Ok(format!("{gutter_blank}--> {path}\n"));
    }
    let file = sources
        .get(span.file)
        .ok_or(SourceError::UnknownFile { file: span.file })?;
    file.span_text(span)?;
    let line_index = file.line_index();
    let (start_line, start_col) = line_index.line_col(span.start)?;
    let mut out = format!(
        "{gutter_blank}--> {}:{start_line}:{start_col}\n{gutter_blank} |\n",
        file.path
    );
    render_source_block(&mut out, line_index, span, gutter_blank, gutter_width)?;
    Ok(out)
}

/// Source-context block for `span`. Shared with `backtrace` so compile and
/// runtime outputs use the same `N | text` / `  | ^^^` shape. Caller owns
/// the `--> file:line:col` header.
pub(crate) fn render_source_block(
    out: &mut String,
    line_index: &LineIndex,
    span: Span,
    gutter_blank: &str,
    gutter_width: usize,
) -> Result<(), SourceError> {
    span.text(line_index.source(), span.file)?;
    let (start_line, start_col) = line_index.line_col(span.start)?;
    let (end_line, end_col) = line_index.line_col(span.end)?;
    let line_count = line_index.line_count();

    if start_line > 1 {
        let prev = line_index.line_text(start_line - 1)?;
        out.push_str(&format!(
            "{:>width$} | {}\n",
            start_line - 1,
            expand_tabs(prev),
            width = gutter_width
        ));
    }

    let start_text = line_index.line_text(start_line)?;
    out.push_str(&format!(
        "{start_line:>gutter_width$} | {}\n",
        expand_tabs(start_text)
    ));
    let caret_indent = display_column(start_text, start_col - 1);
    let caret_len = if start_line == end_line {
        display_column(start_text, end_col - 1)
            .saturating_sub(caret_indent)
            .max(1)
    } else {
        // Multi-line: extend to end of line.
        display_column(start_text, u32::MAX)
            .saturating_sub(caret_indent)
            .max(1)
    };
    out.push_str(&format!(
        "{} | {}{}\n",
        gutter_blank,
        " ".repeat(caret_indent),
        "^".repeat(caret_len)
    ));

    if end_line > start_line {
        for mid_line in (start_line + 1)..end_line {
            let mid_text = line_index.line_text(mid_line)?;
            out.push_str(&format!(
                "{mid_line:>gutter_width$} | {}\n",
                expand_tabs(mid_text)
            ));
            let mid_carets = display_column(mid_text, u32::MAX).max(1);
            out.push_str(&format!("{} | {}\n", gutter_blank, "^".repeat(mid_carets)));
        }
        let end_text = line_index.line_text(end_line)?;
        out.push_str(&format!(
            "{end_line:>gutter_width$} | {}\n",
            expand_tabs(end_text)
        ));
        let end_carets = display_column(end_text, end_col.saturating_sub(1)).max(1);
        out.push_str(&format!("{} | {}\n", gutter_blank, "^".repeat(end_carets)));
    }

    // Skip at EOF; line_count is the last addressable line.
    if end_line < line_count {
        let next = line_index.line_text(end_line + 1)?;
        out.push_str(&format!(
            "{:>width$} | {}\n",
            end_line + 1,
            expand_tabs(next),
            width = gutter_width
        ));
    }
    Ok(())
}

/// Terminal column at a byte offset. Source columns remain byte-based for LSP
/// round trips; only the displayed source and underline use terminal widths.
fn display_column(line: &str, offset: u32) -> usize {
    use unicode_width::UnicodeWidthStr;
    let mut end = (offset as usize).min(line.len());
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    line[..end]
        .split('\t')
        .enumerate()
        .fold(0, |column, (index, text)| {
            let start = if index == 0 {
                column
            } else {
                column + 4 - column % 4
            };
            start + text.width()
        })
}

/// Expand tabs at four-column stops relative to the source, excluding the gutter.
fn expand_tabs(line: &str) -> String {
    use unicode_width::UnicodeWidthStr;
    let mut out = String::new();
    let mut column = 0;
    for (index, text) in line.split('\t').enumerate() {
        if index != 0 {
            let spaces = 4 - column % 4;
            out.push_str(&" ".repeat(spaces));
            column += spaces;
        }
        out.push_str(text);
        column += text.width();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Diagnostic, Severity, render};
    use crate::source::Sources;
    use crate::{FileId, Span};

    const F: FileId = FileId(0);

    fn sources(text: &str) -> Sources {
        let (sources, _) = Sources::single("script.subm", text).unwrap();
        sources
    }

    #[test]
    fn construct_and_read_fields() {
        let d = Diagnostic {
            severity: Severity::Error,
            span: Span::new(F, 10, 20).unwrap(),
            message: "type mismatch".to_string(),
            help: vec![],
            notes: vec![(Span::new(F, 5, 8).unwrap(), "defined here".to_string())],
        };
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.span, Span::new(F, 10, 20).unwrap());
        assert_eq!(d.message, "type mismatch");
        assert_eq!(d.notes.len(), 1);
        assert_eq!(d.notes[0].0, Span::new(F, 5, 8).unwrap());
        assert_eq!(d.notes[0].1, "defined here");
    }

    #[test]
    fn clone_is_equal() {
        let d = Diagnostic {
            severity: Severity::Warning,
            span: Span::new(F, 0, 3).unwrap(),
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
            span: Span::new(F, 27, 34).unwrap(),
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
            span: Span::new(F, start, start + 3).unwrap(),
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
    fn carets_follow_terminal_columns() {
        use unicode_width::UnicodeWidthStr;
        for (source, target, indent, width) in [
            ("let s = \"日本😀\"; bad;", "bad", 18, 3),
            ("\tlet s = \"e\u{301}\";\tbad;", "bad", 20, 3),
            ("x\t日本😀;", "日本😀", 4, 6),
            ("x\tbad;", "\tbad", 1, 6),
        ] {
            let start = source.find(target).unwrap();
            let diag = Diagnostic {
                severity: Severity::Error,
                span: Span::new(F, start as u32, (start + target.len()) as u32).unwrap(),
                message: "bad value".into(),
                help: vec![],
                notes: vec![],
            };
            let rendered = render(&diag, &sources(source));
            assert!(!rendered.contains('\t'), "{rendered}");
            let caret = rendered.lines().find(|line| line.contains('^')).unwrap();
            let expanded = super::expand_tabs(source);
            assert_eq!(caret.find('^').unwrap(), 4 + indent, "{rendered}");
            assert_eq!(caret.matches('^').count(), width, "{rendered}");
            assert_eq!(expanded.width(), super::display_column(source, u32::MAX));
            assert!(rendered.contains(&format!("script.subm:1:{}", start + 1)));
        }
    }

    #[test]
    fn multiline_carets_use_terminal_widths() {
        let source = "a\t日\n\t😀e\u{301}\n終z";
        let end = source.find('終').unwrap() + '終'.len_utf8();
        let diag = Diagnostic {
            severity: Severity::Error,
            span: Span::new(F, 1, end as u32).unwrap(),
            message: "multiline".into(),
            help: vec![],
            notes: vec![],
        };
        let rendered = render(&diag, &sources(source));
        let carets: Vec<_> = rendered.lines().filter(|line| line.contains('^')).collect();
        assert_eq!(
            carets,
            ["  |  ^^^^^", "  | ^^^^^^^", "  | ^^"],
            "{rendered}"
        );
        assert!(!rendered.contains('\t'));
    }

    #[test]
    fn render_multi_note_error_with_gutter_alignment() {
        // 12 lines, each "N\n" padded so line 11 exists to force 2-digit gutter.
        let source = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\n";
        let diag = Diagnostic {
            severity: Severity::Error,
            span: Span::new(F, 2, 3).unwrap(), // "b" on line 2
            message: "missing return in all code paths".to_string(),
            help: vec![],
            notes: vec![
                (
                    Span::new(F, 0, 1).unwrap(),
                    "function declared here".to_string(),
                ),
                (
                    Span::new(F, 20, 21).unwrap(),
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
            span: Span::new(F, 5, 5).unwrap(),
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
            span: Span::new(F, 4, 10).unwrap(),
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
            span: Span::new(F, 0, 5).unwrap(),
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
            span: Span::new(F, 13, 18).unwrap(),
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
            span: Span::new(F, start, end).unwrap(),
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
            span: Span::new(F, 12, 15).unwrap(),
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
            span: Span::new(F, 2, 3).unwrap(),
            message: "bad".to_string(),
            help: vec!["try this instead".to_string()],
            notes: vec![(Span::new(F, 0, 1).unwrap(), "declared here".to_string())],
        };
        insta::assert_snapshot!(render(&diag, &sources(source)));
    }

    #[test]
    fn render_multiple_helps() {
        let source = "x\n";
        let diag = Diagnostic {
            severity: Severity::Error,
            span: Span::new(F, 0, 1).unwrap(),
            message: "bad".to_string(),
            help: vec!["first hint".to_string(), "second hint".to_string()],
            notes: vec![],
        };
        insta::assert_snapshot!(render(&diag, &sources(source)));
    }
}
