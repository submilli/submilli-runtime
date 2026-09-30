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
            render(&warnings, &sources);
            Ok(ExitCode::SUCCESS)
        }
        Err(diags) => {
            render(&diags, &sources);
            Ok(ExitCode::from(1))
        }
    }
}

fn render(diags: &[diagnostics::Diagnostic], sources: &Sources) {
    for d in diags {
        eprint!("{}", diagnostics::render(d, sources));
    }
}
