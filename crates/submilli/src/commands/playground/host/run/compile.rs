//! Parsing and compiling a program, and turning what went wrong into the error a
//! caller sees.
//!
//! Copied from `submilli-server`'s `runner.rs` (parse, compile diagnostics, runtime
//! error classification) and `compiler_thread.rs`.

use interpreter::compiler_limits::COMPILER_STACK_BYTES;
use interpreter::diagnostics::{self, Severity};
use interpreter::runtime::is_memory_exhausted;
use interpreter::{
    BacktraceMode, Diagnostic, FileId, ParsedScript, ScriptImports, Sources, failure_message,
    parse_script,
};
use wasmtime::Trap;

use super::packages::PreparedPackages;
use super::{
    DenialDetails, DiagnosticNote, DiagnosticPayload, ErrorKind, ExecuteError, RunOutcome,
};

/// The name a program's own source is registered under.
pub(super) const FILENAME: &str = "<execute>";

/// A parsed program, with the sources its diagnostics render against.
pub(crate) struct ParsedExecute {
    pub(super) sources: Sources,
    pub(super) file: FileId,
    pub(super) parsed: ParsedScript,
}

impl ParsedExecute {
    /// What the program imports, or the rendered reason its imports cannot be read.
    pub(crate) fn imports(&self) -> Result<ScriptImports, String> {
        self.parsed.external_imports().map_err(|error| {
            let diagnostics = error.into_diagnostics(self.file);
            diagnostics::render_collection(&diagnostics, &self.sources).map_or_else(
                |failure| {
                    let primary = diagnostics
                        .first()
                        .map_or("compilation failed", |diag| diag.message.as_str());
                    interpreter::rendering::failure_text(primary, &failure)
                },
                |rendered| rendered.text,
            )
        })
    }
}

pub(crate) fn parse(code: &str) -> Result<ParsedExecute, interpreter::source::SourceError> {
    let (sources, file) = Sources::single(FILENAME, code)?;
    let parsed = parse_script(code, file);
    Ok(ParsedExecute {
        sources,
        file,
        parsed,
    })
}

/// Why a compile could not run on its own thread.
#[derive(Debug)]
pub(super) enum CompilerThreadError {
    Spawn(std::io::Error),
    Panicked,
}

impl std::fmt::Display for CompilerThreadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(error) => write!(f, "cannot start the compiler thread: {error}"),
            Self::Panicked => f.write_str("the compiler thread panicked"),
        }
    }
}

impl std::error::Error for CompilerThreadError {}

/// Runs `work` on a compiler-sized thread and waits for it, blocking the caller.
/// Compilation recurses with program nesting and needs the interpreter's
/// documented stack, several times a runtime worker's 2 MiB.
pub(super) fn on_compiler_thread<T: Send>(
    work: impl FnOnce() -> T + Send,
) -> Result<T, CompilerThreadError> {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("submilli-compile".into())
            .stack_size(COMPILER_STACK_BYTES)
            .spawn_scoped(scope, work)
            .map_err(CompilerThreadError::Spawn)?
            .join()
            .map_err(|_| CompilerThreadError::Panicked)
    })
}

/// A program that did not compile: the rendered diagnostics, and each one's
/// position.
pub(super) fn compile_failure(
    sources: &Sources,
    diags: &[Diagnostic],
    discovery_warnings: Vec<String>,
) -> RunOutcome {
    let rendered = diagnostics::render_collection_with_limits(
        diags,
        sources,
        interpreter::rendering::RenderLimits {
            bytes: 512 * 1024 - 64,
            ..Default::default()
        },
    );
    let payloads = diagnostic_payloads(sources, diags);
    let (mut message, diagnostics_out, omitted) = match (rendered, payloads) {
        (Ok(rendered), Ok((payloads, omitted))) => (rendered.text, payloads, omitted),
        (Err(error), _) | (_, Err(error)) => {
            let primary = diags.first().map_or("compilation failed", |diagnostic| {
                diagnostic.message.as_str()
            });
            let mut outcome = RunOutcome::internal_failure(&interpreter::rendering::failure_text(
                primary, &error,
            ));
            outcome.discovery_warnings = discovery_warnings;
            return outcome;
        }
    };
    if omitted {
        message.push_str(interpreter::rendering::TRUNCATED);
    }
    RunOutcome {
        error: Some(ExecuteError {
            kind: ErrorKind::CompileError,
            message,
            diagnostics: diagnostics_out,
            denial: None,
        }),
        discovery_warnings,
        ..RunOutcome::default()
    }
}

fn diagnostic_payloads(
    sources: &Sources,
    diagnostics: &[Diagnostic],
) -> Result<(Vec<DiagnosticPayload>, bool), interpreter::rendering::RenderError> {
    use interpreter::rendering::{RenderError, RenderLimits, bounded_text};
    let mut remaining = 512 * 1024usize;
    let mut result = Vec::new();
    let mut omitted = false;
    for diagnostic in diagnostics.iter().take(RenderLimits::default().steps) {
        if remaining < 128 {
            omitted = true;
            break;
        }
        let (line, column) = diagnostic_position(sources, diagnostic.span)?;
        let message = bounded_text(
            &diagnostic.message,
            remaining.min(RenderLimits::default().bytes),
        )?;
        remaining = remaining
            .saturating_sub(message.text.len())
            .saturating_sub(64);
        omitted |= message.truncated;
        let mut notes = Vec::new();
        for (span, text) in diagnostic.notes.iter().take(RenderLimits::default().steps) {
            if remaining < 128 {
                omitted = true;
                break;
            }
            let (line, column) = diagnostic_position(sources, *span)?;
            let message = bounded_text(text, remaining.min(RenderLimits::default().bytes))?;
            remaining = remaining
                .saturating_sub(message.text.len())
                .saturating_sub(64);
            omitted |= message.truncated;
            notes.try_reserve(1).map_err(|_| RenderError::Allocation)?;
            notes.push(DiagnosticNote {
                line,
                column,
                message: message.text,
            });
        }
        omitted |= notes.len() != diagnostic.notes.len();
        result.try_reserve(1).map_err(|_| RenderError::Allocation)?;
        result.push(DiagnosticPayload {
            severity: match diagnostic.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
            },
            line,
            column,
            message: message.text,
            notes,
        });
    }
    omitted |= result.len() != diagnostics.len();
    Ok((result, omitted))
}

fn diagnostic_position(
    sources: &Sources,
    span: interpreter::Span,
) -> Result<(u32, u32), interpreter::source::SourceError> {
    interpreter::Span::new(span.file, span.start, span.end)?;
    if span.file.reserved_path().is_some() {
        return Ok((0, 0));
    }
    let source = sources
        .get(span.file)
        .ok_or(interpreter::source::SourceError::UnknownFile { file: span.file })?;
    source.span_text(span)?;
    source.line_index().line_col(span.start)
}

/// The error a caller sees for a program that failed while it ran.
pub(super) fn classify_runtime_error(
    err: &wasmtime::Error,
    sources: &Sources,
    file: FileId,
) -> ExecuteError {
    let denial = err
        .downcast_ref::<interpreter::backtrace::ThrownError>()
        .and_then(|thrown| thrown.denial.as_ref());
    let kind = if denial.is_some() {
        ErrorKind::PermissionDenied
    } else {
        match err.downcast_ref::<Trap>() {
            Some(Trap::Interrupt) => ErrorKind::Timeout,
            Some(Trap::OutOfFuel) => ErrorKind::FuelExhausted,
            Some(Trap::StackOverflow) => ErrorKind::StackExhausted,
            _ if is_memory_exhausted(err) => ErrorKind::MemoryExhausted,
            _ => ErrorKind::RuntimeError,
        }
    };
    // drops middle host frames; full trace available from the CLI
    let message =
        match interpreter::backtrace::render_checked(err, sources, file, BacktraceMode::LlmTrimmed)
        {
            Ok(Some(rendered)) => rendered.text,
            Ok(None) => unframed_message(err),
            Err(failure) => interpreter::rendering::failure_text(&unframed_message(err), &failure),
        };
    ExecuteError {
        kind,
        message,
        diagnostics: Vec::new(),
        denial: denial.map(|denial| DenialDetails {
            caller: denial.caller.clone(),
            capability: denial.capability.clone(),
            source: denial.source.as_str(),
        }),
    }
}

/// Whether an instantiation error was raised once top-level statements (the
/// program's or a package's) were running, as opposed to a module failing to
/// link: a trap, a shaped throw, a fatal host error, and memory exhaustion.
pub(super) fn raised_by_top_level_statements(err: &wasmtime::Error) -> bool {
    err.is::<Trap>()
        || err.is::<interpreter::backtrace::ThrownError>()
        || err.is::<interpreter::runtime::host::FatalHostError>()
        || is_memory_exhausted(err)
}

/// A trap or an uncaught throw with no frame to render gets only the header a
/// rendered failure has. Any other failure keeps its whole cause chain.
fn unframed_message(err: &wasmtime::Error) -> String {
    if err.is::<Trap>() || err.is::<interpreter::backtrace::ThrownError>() {
        return format!("error: {}", failure_message(err));
    }
    interpreter::backtrace::failure_chain_checked(err).map_or_else(
        |failure| interpreter::rendering::failure_text("runtime execution failed", &failure),
        |rendered| rendered.text,
    )
}

/// Registers every package module so its frames render with their source.
pub(super) fn register_package_sources(
    sources: &mut Sources,
    packages: &PreparedPackages,
) -> Result<(), interpreter::source::SourceError> {
    for package in &packages.modules {
        let name = &package.declaration.package_name;
        for source in &package.sources {
            sources.add_package_module(name, source.path.as_str(), &source.text)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
