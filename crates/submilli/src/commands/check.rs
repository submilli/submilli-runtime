//! `submilli check` — typecheck a `.subm` file without running it. Diagnostics
//! go to stderr; exits non-zero if any error is found. Faster iteration than
//! `run` (no codegen, no execution).

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context;
use interpreter::diagnostics;
use interpreter::{Sources, typecheck};

#[derive(clap::Args)]
pub struct Args {
    /// Path to the `.subm` script to typecheck.
    script: PathBuf,
}

pub fn execute(args: Args) -> anyhow::Result<ExitCode> {
    let source = fs::read_to_string(&args.script)
        .with_context(|| format!("reading {}", args.script.display()))?;
    let filename = args.script.to_string_lossy().into_owned();
    let (sources, file) = Sources::single(filename, source.clone())?;

    match typecheck(&source, file) {
        Ok(warnings) => {
            render(&warnings, &sources)?;
            Ok(ExitCode::SUCCESS)
        }
        Err(diags) => {
            render(&diags, &sources)?;
            Ok(ExitCode::from(1))
        }
    }
}

pub(super) fn render(diags: &[diagnostics::Diagnostic], sources: &Sources) -> anyhow::Result<()> {
    let rendered = diagnostics::render_collection(diags, sources).map_err(|error| {
        let primary = diags
            .first()
            .map_or("compilation failed", |diag| diag.message.as_str());
        anyhow::anyhow!(interpreter::rendering::failure_text(primary, &error))
    })?;
    eprint!("{}", rendered.text);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reporting_failure_retains_the_primary_compiler_error() {
        let (sources, _) = Sources::single("test.ts", "").unwrap();
        let diagnostic = interpreter::Diagnostic {
            severity: interpreter::Severity::Error,
            span: interpreter::Span::at(interpreter::FileId(999)),
            message: "original compiler error".into(),
            help: Vec::new(),
            notes: Vec::new(),
        };
        let error = render(&[diagnostic], &sources).unwrap_err().to_string();
        assert!(error.contains("original compiler error"));
        assert!(error.contains("internal reporting failure"));
        assert!(render(&[], &sources).is_ok());
    }
}
