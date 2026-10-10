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

use crate::rendering::{RenderError, RenderLimits, RenderedText, Writer};

/// Borrowed diagnostic fields for callers with a different owned diagnostic model.
pub struct DiagnosticView<'a> {
    pub severity: Severity,
    pub span: Span,
    pub message: &'a str,
    pub help: &'a [String],
    pub notes: &'a [(Span, String)],
}

impl<'a> From<&'a Diagnostic> for DiagnosticView<'a> {
    fn from(value: &'a Diagnostic) -> Self {
        Self {
            severity: value.severity,
            span: value.span,
            message: &value.message,
            help: &value.help,
            notes: &value.notes,
        }
    }
}

pub fn render(diagnostic: &Diagnostic, sources: &Sources) -> String {
    render_checked(diagnostic, sources).map_or_else(
        |error| crate::rendering::failure_text(&diagnostic.message, &error),
        |rendered| rendered.text,
    )
}

pub fn render_checked(
    diagnostic: &Diagnostic,
    sources: &Sources,
) -> Result<RenderedText, RenderError> {
    render_with_limits(diagnostic, sources, RenderLimits::default())
}

pub fn render_with_limits(
    diagnostic: &Diagnostic,
    sources: &Sources,
    limits: RenderLimits,
) -> Result<RenderedText, RenderError> {
    Writer::render(limits, |out| {
        write_diagnostic(out, &diagnostic.into(), sources)
    })
}

pub fn render_collection(
    diagnostics: &[Diagnostic],
    sources: &Sources,
) -> Result<RenderedText, RenderError> {
    render_collection_with_limits(diagnostics, sources, RenderLimits::collection())
}

pub fn render_collection_with_limits(
    diagnostics: &[Diagnostic],
    sources: &Sources,
    limits: RenderLimits,
) -> Result<RenderedText, RenderError> {
    render_views(
        diagnostics.iter().map(|diagnostic| Ok(diagnostic.into())),
        sources,
        limits,
    )
}

pub fn render_views<'a>(
    diagnostics: impl IntoIterator<Item = Result<DiagnosticView<'a>, RenderError>>,
    sources: &Sources,
    limits: RenderLimits,
) -> Result<RenderedText, RenderError> {
    Writer::render(limits, |out| {
        for diagnostic in diagnostics {
            out.step()?;
            let diagnostic = diagnostic?;
            let rendered = Writer::render(out.child_limits(), |child| {
                write_diagnostic(child, &diagnostic, sources)
            })?;
            out.append(rendered)?;
        }
        Ok(())
    })
}

fn write_diagnostic(
    out: &mut Writer,
    diagnostic: &DiagnosticView<'_>,
    sources: &Sources,
) -> Result<(), RenderError> {
    validate_span(sources, diagnostic.span)?;
    let severity = match diagnostic.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    };
    let mut max_line = 1;
    let validation_allowance = out.remaining_steps() / 2;
    let mut validation_truncated = false;
    for (visited, span) in std::iter::once(diagnostic.span)
        .chain(diagnostic.notes.iter().map(|(span, _)| *span))
        .enumerate()
    {
        if visited >= validation_allowance {
            validation_truncated = true;
            break;
        }
        out.step()?;
        validate_span(sources, span)?;
        if let Some(source) = sources.get(span.file) {
            let index = source.line_index();
            let (line, _) = index.line_col(span.end)?;
            max_line = max_line.max(line.saturating_add(1).min(index.line_count()));
        }
    }
    out.format(format_args!("{severity}: {}\n", diagnostic.message))?;
    if validation_truncated {
        return Err(RenderError::Truncated);
    }
    let width = max_line.to_string().len();
    write_context(out, sources, diagnostic.span, width)?;
    for help in diagnostic.help {
        out.format(format_args!("{:width$} |\nhelp: {help}\n", ""))?;
    }
    for (span, message) in diagnostic.notes {
        out.format(format_args!("{:width$} |\nnote: {message}\n", ""))?;
        write_context(out, sources, *span, width)?;
    }
    Ok(())
}

pub(crate) fn validate_span(sources: &Sources, span: Span) -> Result<(), SourceError> {
    Span::new(span.file, span.start, span.end)?;
    if span.file.reserved_path().is_some() {
        return Ok(());
    }
    sources
        .get(span.file)
        .ok_or(SourceError::UnknownFile { file: span.file })?
        .span_text(span)?;
    Ok(())
}

fn write_context(
    out: &mut Writer,
    sources: &Sources,
    span: Span,
    width: usize,
) -> Result<(), RenderError> {
    if let Some(path) = span.file.reserved_path() {
        return out.format(format_args!("{:width$}--> {path}\n", ""));
    }
    let source = sources
        .get(span.file)
        .ok_or(SourceError::UnknownFile { file: span.file })?;
    let index = source.line_index();
    let (line, col) = index.line_col(span.start)?;
    out.format(format_args!(
        "{:width$}--> {}:{line}:{col}\n{:width$} |\n",
        "", source.path, ""
    ))?;
    write_source_block(out, index, span, width)
}

pub(crate) fn write_source_block(
    out: &mut Writer,
    index: &LineIndex,
    span: Span,
    width: usize,
) -> Result<(), RenderError> {
    span.text(index.source(), span.file)?;
    let (start, start_col) = index.line_col(span.start)?;
    let (end, end_col) = index.line_col(span.end)?;
    if start > 1 {
        write_line(out, index, start - 1, width)?;
    }
    for line in start..=end {
        out.step()?;
        let text = index.line_text(line)?;
        write_line(out, index, line, width)?;
        let indent = if line == start {
            display_column(out, text, start_col.saturating_sub(1))?
        } else {
            0
        };
        let last = if line == end {
            end_col.saturating_sub(1)
        } else {
            u32::MAX
        };
        let carets = display_column(out, text, last)?
            .saturating_sub(indent)
            .max(1);
        out.format(format_args!("{:width$} | ", ""))?;
        out.repeat(' ', indent)?;
        out.repeat('^', carets)?;
        out.push("\n")?;
    }
    if end < index.line_count() {
        write_line(out, index, end + 1, width)?;
    }
    Ok(())
}

fn write_line(
    out: &mut Writer,
    index: &LineIndex,
    line: u32,
    width: usize,
) -> Result<(), RenderError> {
    use unicode_width::UnicodeWidthStr;
    out.format(format_args!("{line:>width$} | "))?;
    let text = index.line_text(line)?;
    let mut column = 0usize;
    for (i, segment) in text.split('\t').enumerate() {
        out.step()?;
        if i != 0 {
            let spaces = 4 - column % 4;
            out.repeat(' ', spaces)?;
            column = column.checked_add(spaces).ok_or(RenderError::Formatting)?;
        }
        // The byte limit is checked before scanning potentially huge text for width.
        out.push(segment)?;
        column = column
            .checked_add(segment.width())
            .ok_or(RenderError::Formatting)?;
    }
    out.push("\n")
}

fn display_column(out: &mut Writer, line: &str, offset: u32) -> Result<usize, RenderError> {
    use unicode_width::UnicodeWidthStr;
    let mut end = (offset as usize).min(line.len());
    while !line.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    let text = line.get(..end).ok_or(RenderError::Formatting)?;
    let mut column = 0usize;
    for (i, segment) in text.split('\t').enumerate() {
        out.step()?;
        if i != 0 {
            column = column
                .checked_add(4 - column % 4)
                .ok_or(RenderError::Formatting)?;
        }
        // A line successfully emitted above is already bounded by the output limit.
        column = column
            .checked_add(segment.width())
            .ok_or(RenderError::Formatting)?;
    }
    Ok(column)
}

/// Individually rendered warnings with a shared collection byte/work allowance.
pub fn render_list(
    diagnostics: &[Diagnostic],
    sources: &Sources,
) -> Result<Vec<String>, RenderError> {
    let mut result = Vec::new();
    let mut remaining = RenderLimits::collection().bytes;
    let mut steps = RenderLimits::collection().steps;
    for diagnostic in diagnostics.iter().take(RenderLimits::collection().steps) {
        if remaining < crate::rendering::TRUNCATED.len() * 2 || steps == 0 {
            break;
        }
        let rendered = render_with_limits(
            diagnostic,
            sources,
            RenderLimits {
                bytes: remaining
                    .min(RenderLimits::default().bytes)
                    .saturating_sub(crate::rendering::TRUNCATED.len()),
                steps,
                ..RenderLimits::default()
            },
        )?;
        steps = steps.saturating_sub(rendered.steps());
        remaining = remaining.saturating_sub(rendered.text.len());
        result.try_reserve(1).map_err(|_| RenderError::Allocation)?;
        result.push(rendered.text);
    }
    if result.len() < diagnostics.len() {
        result.try_reserve(1).map_err(|_| RenderError::Allocation)?;
        result.push(crate::rendering::TRUNCATED.into());
    }
    Ok(result)
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
            assert_eq!(caret.find('^').unwrap(), 4 + indent, "{rendered}");
            assert_eq!(caret.matches('^').count(), width, "{rendered}");
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
