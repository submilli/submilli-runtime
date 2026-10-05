//! Trap → Submilli-source backtrace rendering.

use wasmtime::{Error, FrameInfo, Trap, WasmBacktrace};

use crate::diagnostics::write_source_block;
use crate::rendering::{RenderError, RenderLimits, RenderedText, Writer};
use crate::source::Sources;
use crate::{FileId, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BacktraceMode {
    Full,
    /// Every user frame plus the innermost and outermost non-user frame.
    /// Middle non-user frames are dropped — boundary frames anchor where
    /// user code crossed into the runtime without exposing prelude internals.
    LlmTrimmed,
}

/// An uncaught thrown `Error`, carrying its `name: message` text plus the
/// throw-site backtrace the engine attached to the escaped exception;
/// `uncaught_error` re-wraps both here so [`render`] prints a source
/// backtrace like a trap.
#[derive(Debug)]
pub struct ThrownError {
    pub message: String,
    pub backtrace: Option<WasmBacktrace>,
}

impl std::fmt::Display for ThrownError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ThrownError {}

/// Render the backtrace against `sources`. `file` is the entrypoint script; the
/// registry may also include package sources whose DWARF paths can render
/// source context for package frames.
pub fn render(
    error: &Error,
    sources: &Sources,
    file: FileId,
    mode: BacktraceMode,
) -> Option<String> {
    match render_checked(error, sources, file, mode) {
        Ok(rendered) => rendered.map(|text| text.text),
        Err(failure) => Some(crate::rendering::failure_text(
            &failure_message(error),
            &failure,
        )),
    }
}

pub fn render_checked(
    error: &Error,
    sources: &Sources,
    file: FileId,
    mode: BacktraceMode,
) -> Result<Option<RenderedText>, RenderError> {
    let (trace, label) = if let Some(thrown) = error.downcast_ref::<ThrownError>() {
        (thrown.backtrace.as_ref(), "thrown here")
    } else {
        (error.downcast_ref::<WasmBacktrace>(), trap_label_for(error))
    };
    let Some(trace) = trace else {
        return Ok(None);
    };
    sources
        .get(file)
        .ok_or(crate::source::SourceError::UnknownFile { file })?;
    let mut has_frame = false;
    let rendered = Writer::render(RenderLimits::default(), |out| {
        out.push("error: ")?;
        write_failure(out, error)?;
        out.push("\n")?;
        let mut first = None;
        let mut last = None;
        for (i, frame) in trace.frames().iter().enumerate() {
            out.step()?;
            if frame.symbols().first().and_then(|s| s.name()).is_none() {
                continue;
            }
            if !is_source_frame(frame, sources) {
                first.get_or_insert(i);
                last = Some(i);
            }
        }
        let mut rendered_index = 0usize;
        // No temporary frame vectors: both scans share the work budget.
        let mut last_kept = None;
        for (i, frame) in trace.frames().iter().enumerate() {
            out.step()?;
            if frame.symbols().first().and_then(|s| s.name()).is_none() {
                continue;
            }
            if mode == BacktraceMode::Full
                || is_source_frame(frame, sources)
                || Some(i) == first
                || Some(i) == last
            {
                last_kept = Some(i);
            }
        }
        for (i, frame) in trace.frames().iter().enumerate() {
            out.step()?;
            if frame.symbols().first().and_then(|s| s.name()).is_none() {
                continue;
            }
            if mode == BacktraceMode::LlmTrimmed
                && !is_source_frame(frame, sources)
                && Some(i) != first
                && Some(i) != last
            {
                continue;
            }
            let role = if rendered_index == 0 {
                label
            } else if Some(i) == last_kept {
                "entry"
            } else {
                "caller"
            };
            write_frame(out, frame, sources, role)?;
            rendered_index = rendered_index
                .checked_add(1)
                .ok_or(RenderError::Formatting)?;
            has_frame = true;
        }
        Ok(())
    })?;
    Ok((has_frame || rendered.truncated).then_some(rendered))
}

pub fn failure_message_checked(error: &Error) -> Result<RenderedText, RenderError> {
    Writer::render(RenderLimits::default(), |out| write_failure(out, error))
}

fn write_failure(out: &mut Writer, error: &Error) -> Result<(), RenderError> {
    match error.downcast_ref::<Trap>() {
        Some(Trap::Interrupt) => out.push("timeout exceeded"),
        Some(Trap::OutOfFuel) => out.push("fuel exhausted"),
        Some(Trap::UnreachableCodeReached) => out.push("unreachable code reached"),
        _ => out.format(format_args!("{error}")),
    }
}

/// The one-line text for a run's failure: the header [`render`] puts above the
/// frames, and what a caller prints when `render` has no frames to show.
///
/// Most engine messages already say what the program did (`null reference`,
/// `cast failure`, `out of bounds array access`) and are used verbatim. These
/// three name an engine mechanism instead — `interrupt` and
/// `all fuel consumed by WebAssembly` tell a reader nothing they can act on, and
/// leak the host into a diagnostic that is supposed to be about their code.
///
/// Deliberately not the same table as [`trap_label_for`]: a stack overflow reads
/// better as the engine's `call stack exhausted` in the header (it says what
/// happened) and as `stack overflow` in the frame label (it names the frame).
pub fn failure_message(error: &Error) -> String {
    failure_message_checked(error).map_or_else(
        |failure| crate::rendering::failure_text("runtime execution failed", &failure),
        |rendered| rendered.text,
    )
}

fn trap_label_for(error: &Error) -> &'static str {
    match error.downcast_ref::<Trap>() {
        Some(Trap::UnreachableCodeReached) => "unreachable code reached",
        Some(Trap::OutOfFuel) => "fuel exhausted",
        Some(Trap::StackOverflow) => "stack overflow",
        Some(Trap::Interrupt) => "timeout exceeded",
        Some(_) | None => "trap raised here",
    }
}

fn write_frame(
    out: &mut Writer,
    frame: &FrameInfo,
    sources: &Sources,
    role: &str,
) -> Result<(), RenderError> {
    let Some(sym) = frame.symbols().first() else {
        return Ok(());
    };
    let Some(name) = sym.name() else {
        return Ok(());
    };
    let path = sym.file().unwrap_or("?");
    let line = sym.line().unwrap_or(0);
    let col = sym.column().unwrap_or(0);
    write_symbol(out, name, path, line, col, sources, role)
}

fn write_symbol(
    out: &mut Writer,
    name: &str,
    path: &str,
    line: u32,
    col: u32,
    sources: &Sources,
    role: &str,
) -> Result<(), RenderError> {
    let context = if let Some((file, source)) = sources.find_path(path)
        && line != 0
    {
        let index = source.line_index();
        let offset = index.byte_offset(line, col.max(1))?;
        Some((index, Span::new(file, offset, offset)?))
    } else {
        None
    };
    out.format(format_args!(
        "  at {name} ({path}:{line}:{col})  [{role}]\n"
    ))?;
    if let Some((index, span)) = context {
        let width = index.line_count().max(1).to_string().len();
        write_source_block(out, index, span, width)?;
    }
    Ok(())
}

fn is_source_frame(frame: &FrameInfo, sources: &Sources) -> bool {
    frame
        .symbols()
        .first()
        .and_then(|s| s.file())
        .is_some_and(|p| sources.find_path(p).is_some())
}

/// The engine delegates alternate Display to anyhow's cause iterator. Its writes
/// pass through the same bounded sink, including separators between causes.
pub fn failure_chain_checked(error: &Error) -> Result<RenderedText, RenderError> {
    Writer::render(RenderLimits::default(), |out| {
        out.format(format_args!("{error:#}"))
    })
}

/// Returns every user frame plus the first and last non-user frame.
#[cfg(test)]
fn kept_indices(is_user: &[bool]) -> Vec<usize> {
    let first_non_user = is_user.iter().position(|u| !u);
    let last_non_user = is_user.iter().rposition(|u| !u);
    (0..is_user.len())
        .filter(|i| is_user[*i] || Some(*i) == first_non_user || Some(*i) == last_non_user)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_symbol_context_fails_before_a_long_frame_can_truncate() {
        let (sources, _) = Sources::single("test.ts", "x").unwrap();
        let long_name = "frame".repeat(20_000);
        let result = Writer::render(RenderLimits::default(), |out| {
            write_symbol(out, &long_name, "test.ts", 99, 1, &sources, "caller")
        });
        assert!(matches!(result, Err(RenderError::Source(_))));
        let valid = Writer::render(RenderLimits::default(), |out| {
            write_symbol(out, &long_name, "test.ts", 1, 1, &sources, "caller")
        })
        .unwrap();
        assert!(valid.truncated);
        let unknown = Writer::render(RenderLimits::default(), |out| {
            write_symbol(out, "frame", "unknown.ts", 99, 1, &sources, "caller")
        })
        .unwrap();
        assert!(unknown.text.contains("unknown.ts:99:1"));
    }

    #[test]
    fn oversized_failure_chain_keeps_its_primary_message() {
        let error = Error::msg("primary failure ".repeat(20_000));
        let result = failure_chain_checked(&error).unwrap();
        assert!(result.truncated);
        assert!(result.text.starts_with("primary failure"));
        assert!(result.text.len() <= RenderLimits::default().bytes);
    }

    #[test]
    fn curated_trap_messages_replace_the_engines_wording() {
        // `Interrupt` and `OutOfFuel` name a host mechanism; a reader can act on
        // neither. The rest of the table already describes what the program did.
        for (trap, expected) in [
            (Trap::Interrupt, "timeout exceeded"),
            (Trap::OutOfFuel, "fuel exhausted"),
            (Trap::UnreachableCodeReached, "unreachable code reached"),
        ] {
            assert_eq!(failure_message(&Error::new(trap)), expected);
        }
    }

    #[test]
    fn uncurated_traps_keep_the_engine_message() {
        assert_eq!(
            failure_message(&Error::new(Trap::NullReference)),
            "null reference",
        );
    }

    fn run(is_user: &[bool]) -> Vec<bool> {
        kept_indices(is_user)
            .into_iter()
            .map(|i| is_user[i])
            .collect()
    }

    #[test]
    fn empty_in_empty_out() {
        assert!(run(&[]).is_empty());
    }

    #[test]
    fn all_user_unchanged() {
        assert_eq!(run(&[true]), vec![true]);
        assert_eq!(run(&[true, true, true]), vec![true, true, true]);
    }

    #[test]
    fn single_non_user_kept() {
        assert_eq!(run(&[false]), vec![false]);
    }

    #[test]
    fn one_non_user_then_users() {
        assert_eq!(run(&[false, true, true]), vec![false, true, true]);
    }

    #[test]
    fn middle_non_user_dropped() {
        assert_eq!(
            run(&[false, false, false, true, true]),
            vec![false, false, true, true],
        );
    }

    #[test]
    fn interleaved_non_user_keeps_anchors_only() {
        assert_eq!(
            run(&[false, true, false, true, false]),
            vec![false, true, true, false],
        );
    }

    #[test]
    fn all_non_user_keeps_first_and_last() {
        assert_eq!(run(&[false, false, false]), vec![false, false]);
        assert_eq!(run(&[false, false, false, false]), vec![false, false],);
    }
}
