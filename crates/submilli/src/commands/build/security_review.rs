//! Static package reviews run through installed coding-agent CLIs.

mod agent;
mod authority;
mod report;
mod snapshot;

use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, bail};
use clap::ValueEnum;
use serde::Serialize;

use agent::Agent;
use report::{Report, Severity};

const SKILL: &str = include_str!("../../../../../skills/submilli-security-review/SKILL.md");

#[derive(clap::Args)]
pub struct Args {
    /// Installed coding agent to invoke; uses its existing authentication.
    #[arg(short = 'a', long, value_enum)]
    agent: Agent,
    /// Provider model ID; "astra" resolves to gpt-6-astra for Codex/Copilot.
    #[arg(short = 'm', long)]
    model: String,
    /// Reasoning effort; the selected CLI/model must support it.
    #[arg(short = 'e', long, value_enum)]
    effort: Option<Effort>,
    /// Review this package and its local dependency closure; default: all packages.
    #[arg(short = 'p', long)]
    pub(super) package: Option<String>,
    /// Fail on findings at this severity or higher. Incomplete reviews always fail.
    #[arg(long, value_enum, default_value = "high")]
    fail_on: Severity,
    /// Write a JSON report, including failures. Refuses to replace an existing file.
    #[arg(long, value_name = "FILE")]
    output: Option<PathBuf>,
    /// Maximum time for the agent, in seconds (1–3600).
    #[arg(long, default_value_t = 600, value_parser = clap::value_parser!(u64).range(1..=3600))]
    timeout: u64,
}

#[derive(Clone, Copy, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
enum Effort {
    Low,
    Medium,
    High,
}

impl Effort {
    fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

pub fn execute(args: Args) -> anyhow::Result<ExitCode> {
    match execute_review(args) {
        Ok(code) => Ok(code),
        Err(error) => {
            eprintln!("Review error: {error:#}");
            Ok(ExitCode::from(2))
        }
    }
}

fn execute_review(args: Args) -> anyhow::Result<ExitCode> {
    let mut destination = args.output.as_ref().map(open_report).transpose()?;
    let mut report = Report::pending(&args);
    // Reserve a fresh report before any work, so an interrupted run cannot leave
    // an earlier successful report masquerading as this invocation's result.
    write_report(&mut destination, &report)?;
    if let Err(error) = review(&args, &mut report) {
        report.fail(format!("{error:#}"));
    }
    write_report(&mut destination, &report)?;
    report.print();
    Ok(ExitCode::from(report.exit_code(args.fail_on)))
}

fn open_report(path: &PathBuf) -> anyhow::Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| {
            format!(
                "create new report {}; use a fresh output path",
                path.display()
            )
        })
}

fn write_report(destination: &mut Option<File>, report: &Report) -> anyhow::Result<()> {
    if let Some(file) = destination {
        let bytes = serde_json::to_vec_pretty(report)?;
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        file.set_len(u64::try_from(bytes.len())?.saturating_add(1))?;
        file.flush()?;
    }
    Ok(())
}

fn review(args: &Args, report: &mut Report) -> anyhow::Result<()> {
    if args.model.trim().is_empty() || args.model.chars().any(char::is_control) {
        bail!("model must be a nonempty model identifier");
    }
    if args.model == "astra" && matches!(args.agent, Agent::Claude) {
        bail!("astra is a Codex/Copilot alias; select a Claude model for --agent claude");
    }
    let (mut snapshot, manifest) = snapshot::collect(args.package.as_deref())?;
    report.set_snapshot(&snapshot)?;
    if snapshot.coverage_gaps.is_empty() {
        snapshot.authority = Some(authority::collect(
            &snapshot,
            &manifest,
            args.package.as_deref(),
        )?);
        report.set_snapshot(&snapshot)?;
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("start security review process runtime")?;
    let (response, version) = runtime.block_on(agent::run(
        args,
        &snapshot,
        Duration::from_secs(args.timeout),
    ))?;
    report.agent_version = Some(version);
    report.accept(response, &snapshot)
}
