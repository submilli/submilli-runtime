//! Trap → Submilli-source backtrace rendering.

use wasmtime::{Error, FrameInfo, FrameSymbol, Trap, WasmBacktrace};

use crate::diagnostics::render_source_block;
use crate::source::Sources;
use crate::{FileId, LineIndex, Span};

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
/// `map_uncaught_exception` re-wraps both here so [`render`] prints a source
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
    sources.get(file)?;

    // A thrown `Error` carries its message plus a backtrace we captured at throw
    // time. Render a message header, then the frames in the trap layout. With no
    // captured backtrace, return `None` so the caller falls back to `error: {err}`.
    if let Some(thrown) = error.downcast_ref::<ThrownError>() {
        let frames = render_frames(thrown.backtrace.as_ref()?, sources, mode, "thrown here")?;
        return Some(format!("error: {}\n{frames}", thrown.message));
    }
    // A raw trap carries its message only on the error value — unlike a thrown
    // error, whose text also sits in the frames. Without this header the message
    // is lost, and a trap stops rendering like a compile error.
    let frames = render_frames(
        error.downcast_ref::<WasmBacktrace>()?,
        sources,
        mode,
        trap_label_for(error),
    )?;
    Some(format!("error: {}\n{frames}", trap_message_for(error)))
}

/// Header text for a trap. Most engine messages already say what the program did
/// (`null reference`, `cast failure`, `out of bounds array access`) and are used
/// verbatim. These three name an engine mechanism instead — `interrupt` and
/// `all fuel consumed by WebAssembly` tell a reader nothing they can act on, and
/// leak the host into a diagnostic that is supposed to be about their code.
///
/// Deliberately not the same table as [`trap_label_for`]: a stack overflow reads
/// better as the engine's `call stack exhausted` in the header (it says what
/// happened) and as `stack overflow` in the frame label (it names the frame).
fn trap_message_for(error: &Error) -> String {
    match error.downcast_ref::<Trap>() {
        Some(Trap::Interrupt) => "timeout exceeded".to_string(),
        Some(Trap::OutOfFuel) => "fuel exhausted".to_string(),
        Some(Trap::UnreachableCodeReached) => "unreachable code reached".to_string(),
        Some(_) | None => error.to_string(),
    }
}

fn render_frames(
    bt: &WasmBacktrace,
    sources: &Sources,
    mode: BacktraceMode,
    frame0_label: &'static str,
) -> Option<String> {
    // Filter before role assignment so labels attach to rendered indices, not raw ones.
    // Stack-overflow traps land in frames with no `symbols()`; without this, the
    // innermost rendered frame would be tagged `[caller]` and the trap label dropped.
    let renderable: Vec<&FrameInfo> = bt
        .frames()
        .iter()
        .filter(|f| f.symbols().first().and_then(|s| s.name()).is_some())
        .collect();

    let renderable = match mode {
        BacktraceMode::Full => renderable,
        BacktraceMode::LlmTrimmed => {
            let flags: Vec<bool> = renderable
                .iter()
                .map(|f| is_source_frame(f, sources))
                .collect();
            let kept = kept_indices(&flags);
            kept.into_iter().map(|i| renderable[i]).collect()
        }
    };

    let total = renderable.len();
    let mut out = String::new();
    for (i, frame) in renderable.iter().enumerate() {
        let role = role_for(i, total, frame0_label);
        render_frame(&mut out, frame, sources, role);
    }
    if out.is_empty() { None } else { Some(out) }
}

fn role_for(i: usize, total: usize, trap_label: &'static str) -> &'static str {
    if i == 0 {
        trap_label
    } else if i + 1 == total {
        "entry"
    } else {
        "caller"
    }
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

#[allow(clippy::too_many_arguments)]
fn render_frame(out: &mut String, frame: &FrameInfo, sources: &Sources, role: &str) {
    let Some(sym) = frame.symbols().first() else {
        return;
    };
    let Some(name) = sym.name() else {
        return;
    };
    let frame_file = sym.file().unwrap_or("?");
    let line = sym.line().unwrap_or(0);
    let col = sym.column().unwrap_or(0);

    out.push_str(&format!(
        "  at {name} ({frame_file}:{line}:{col})  [{role}]\n"
    ));

    if let Some((file, source_file)) = sources.find_path(frame_file) {
        let gutter_width = source_file
            .line_index()
            .line_count()
            .max(1)
            .to_string()
            .len();
        let gutter_blank = " ".repeat(gutter_width);
        emit_context(
            out,
            source_file.line_index(),
            file,
            sym,
            &gutter_blank,
            gutter_width,
        );
    }
}

fn emit_context(
    out: &mut String,
    line_index: &LineIndex,
    file: FileId,
    sym: &FrameSymbol,
    gutter_blank: &str,
    gutter_width: usize,
) {
    let line = sym.line().unwrap_or(0);
    let col = sym.column().unwrap_or(0);
    if line == 0 || line > line_index.line_count() {
        return;
    }
    // DWARF gives us (line, col); synthesise a one-byte span at that
    // position so we can share the diagnostic renderer's ±1 + caret
    // logic. `col == 0` means "no column info" → put the caret at
    // column 1 rather than dropping the block entirely.
    let caret_col = col.max(1);
    let context = (|| {
        let offset = line_index.byte_offset(line, caret_col)?;
        let span = Span::new(file, offset, offset)?;
        let mut context = String::new();
        render_source_block(&mut context, line_index, span, gutter_blank, gutter_width)?;
        Ok::<_, crate::source::SourceError>(context)
    })();
    match context {
        Ok(context) => out.push_str(&context),
        Err(error) => out.push_str(&format!(
            "{gutter_blank} | source context unavailable: {error}\n"
        )),
    }
}

fn is_source_frame(frame: &FrameInfo, sources: &Sources) -> bool {
    frame
        .symbols()
        .first()
        .and_then(|s| s.file())
        .is_some_and(|p| sources.find_path(p).is_some())
}

/// Returns indices for `LlmTrimmed`: every user frame plus the first and last non-user frame.
fn kept_indices(is_user: &[bool]) -> Vec<usize> {
    let first_non_user = is_user.iter().position(|u| !u);
    let last_non_user = is_user.iter().rposition(|u| !u);
    (0..is_user.len())
        .filter(|i| is_user[*i] || Some(*i) == first_non_user || Some(*i) == last_non_user)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{kept_indices, trap_message_for};
    use wasmtime::{Error, Trap};

    #[test]
    fn curated_trap_messages_replace_the_engines_wording() {
        // `Interrupt` and `OutOfFuel` name a host mechanism; a reader can act on
        // neither. The rest of the table already describes what the program did.
        for (trap, expected) in [
            (Trap::Interrupt, "timeout exceeded"),
            (Trap::OutOfFuel, "fuel exhausted"),
            (Trap::UnreachableCodeReached, "unreachable code reached"),
        ] {
            assert_eq!(trap_message_for(&Error::new(trap)), expected);
        }
    }

    #[test]
    fn uncurated_traps_keep_the_engine_message() {
        assert_eq!(
            trap_message_for(&Error::new(Trap::NullReference)),
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
