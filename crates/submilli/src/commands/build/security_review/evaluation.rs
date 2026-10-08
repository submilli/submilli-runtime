//! Opt-in developer evaluation. No evaluation flags enter the production CLI.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, bail, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{Args, Effort, SKILL, agent, authority, report, snapshot};

#[cfg(unix)]
mod tests;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    fixtures: BTreeMap<String, PathBuf>,
    trials: Vec<Trial>,
    output: PathBuf,
    prepare_only: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Trial {
    id: String,
    fixture: String,
    arm: Arm,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Arm {
    SourceOnly,
    SourcePlusMap,
}

struct Prepared {
    snapshot: snapshot::Snapshot,
    elapsed: f64,
}

#[test]
#[ignore = "explicit developer evaluation; invokes authenticated Codex and writes external artifacts"]
fn run_evaluation() -> anyhow::Result<()> {
    let path = std::env::var_os("SUBMILLI_REVIEW_EVAL_CONFIG")
        .context("set SUBMILLI_REVIEW_EVAL_CONFIG using scripts/security-review-eval/run.py")?;
    let bytes = fs::read(path)?;
    let config: Config = serde_json::from_slice(&bytes)?;
    validate(&config)?;
    // The runner reserves the directory and records suite metadata before this
    // process starts. A fresh config marker prevents reuse of an earlier run.
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(config.output.join("config.json"))?
        .write_all(&bytes)?;
    let prepared = prepare(&config)?;
    if config.prepare_only {
        return Ok(());
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(run_trials(&config, prepared))
}

fn validate(config: &Config) -> anyhow::Result<()> {
    ensure!(!config.fixtures.is_empty(), "no fixtures selected");
    ensure!(!config.trials.is_empty(), "no trials selected");
    let mut ids = BTreeSet::new();
    for trial in &config.trials {
        ensure!(
            !trial.id.is_empty()
                && trial
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                && ids.insert(&trial.id),
            "invalid or duplicate trial ID"
        );
        ensure!(
            config.fixtures.contains_key(&trial.fixture),
            "unknown fixture"
        );
    }
    Ok(())
}

fn prepare(config: &Config) -> anyhow::Result<BTreeMap<String, Prepared>> {
    let mut prepared = BTreeMap::new();
    for (id, root) in &config.fixtures {
        let started = Instant::now();
        let (mut snapshot, manifest) = snapshot::collect_from(root, None)?;
        ensure!(
            snapshot.coverage_gaps.is_empty(),
            "fixture {id} has missing source"
        );
        snapshot.authority = Some(authority::collect(&snapshot, &manifest, None)?);
        prepared.insert(
            id.clone(),
            Prepared {
                snapshot,
                elapsed: started.elapsed().as_secs_f64(),
            },
        );
    }
    let fingerprints: BTreeMap<_, _> = prepared
        .iter()
        .map(|(id, p)| {
            Ok((
                id,
                json!({
                    "source_sha256": p.snapshot.source_hash()?,
                    "authority_sha256": p.snapshot.authority.as_ref().map(|map| &map.sha256),
                    "preparation_seconds": p.elapsed,
                }),
            ))
        })
        .collect::<anyhow::Result<_>>()?;
    write_json(&config.output.join("prepared.json"), &json!(fingerprints))?;
    Ok(prepared)
}

async fn run_trials(
    config: &Config,
    mut prepared: BTreeMap<String, Prepared>,
) -> anyhow::Result<()> {
    let args = review_args();
    for (index, trial) in config.trials.iter().enumerate() {
        println!("Review {}/{}: {}", index + 1, config.trials.len(), trial.id);
        let fixture = prepared
            .get_mut(&trial.fixture)
            .context("unprepared fixture")?;
        let directory = config.output.join(&trial.id);
        fs::create_dir(&directory)?;
        // Both arms share the captured bytes and one compiler result. Withhold
        // the entire map (including generated schemas) from the source-only arm.
        let withheld = match trial.arm {
            Arm::SourceOnly => fixture.snapshot.authority.take(),
            Arm::SourcePlusMap => None,
        };
        let result = run_trial(&args, &fixture.snapshot, &directory, fixture.elapsed).await;
        if withheld.is_some() {
            fixture.snapshot.authority = withheld;
        }
        result?;
    }
    Ok(())
}

async fn run_trial(
    args: &Args,
    snapshot: &snapshot::Snapshot,
    directory: &Path,
    preparation_seconds: f64,
) -> anyhow::Result<()> {
    let mut report = report::Report::pending(args);
    report.set_snapshot(snapshot)?;
    write_json(
        &directory.join("report.json"),
        &serde_json::to_value(&report)?,
    )?;
    let started = Instant::now();
    let result = agent::run_in(
        args,
        snapshot,
        Duration::from_secs(args.timeout),
        directory,
        true,
    )
    .await;
    let reviewer_seconds = started.elapsed().as_secs_f64();
    let was_interrupted = result
        .as_ref()
        .is_err_and(anyhow::Error::is::<agent::ReviewInterrupted>);
    match result {
        Ok((response, version)) => {
            report.agent_version = Some(version);
            if let Err(error) = report.accept(response, snapshot) {
                report.fail(format!("{error:#}"));
            }
        }
        Err(error) => report.fail(format!("{error:#}")),
    }
    write_json(
        &directory.join("report.json"),
        &serde_json::to_value(&report)?,
    )?;
    let usage = fs::read_to_string(directory.join("stdout"))
        .map_err(anyhow::Error::from)
        .and_then(|text| codex_usage(&text));
    let (usage, usage_error) = match usage {
        Ok(Some(usage)) => (Some(usage), None),
        Ok(None) => (None, Some("Codex did not report token usage".to_owned())),
        Err(error) => (None, Some(format!("{error:#}"))),
    };
    let prompt = fs::read(directory.join("prompt.txt")).ok();
    write_json(
        &directory.join("metrics.json"),
        &json!({
            "schema_version": 1,
            "prompt_sha256": prompt.as_ref().map(|p| hash(p)),
            "skill_sha256": hash(SKILL.as_bytes()),
            "preparation_seconds": preparation_seconds,
            "reviewer_seconds": reviewer_seconds,
            "usage": usage,
            "usage_error": usage_error,
        }),
    )?;
    if was_interrupted {
        return Err(agent::ReviewInterrupted.into());
    }
    Ok(())
}

fn review_args() -> Args {
    Args {
        agent: agent::Agent::Codex,
        model: "gpt-6-astra".to_owned(),
        effort: Some(Effort::High),
        package: None,
        fail_on: report::Severity::High,
        output: None,
        timeout: 600,
    }
}

fn codex_usage(events: &str) -> anyhow::Result<Option<Value>> {
    let mut usage = None;
    for line in events.lines().filter(|line| !line.trim().is_empty()) {
        let event: Value = serde_json::from_str(line).context("invalid Codex JSONL")?;
        if event["type"] != "turn.completed" {
            continue;
        }
        ensure!(
            usage.is_none(),
            "multiple completed turns in a single review"
        );
        let reported = &event["usage"];
        for key in ["input_tokens", "cached_input_tokens", "output_tokens"] {
            ensure!(reported[key].as_u64().is_some(), "missing or invalid {key}");
        }
        if reported["cached_input_tokens"].as_u64() > reported["input_tokens"].as_u64() {
            bail!("cached input exceeds input tokens");
        }
        usage = Some(reported.clone());
    }
    Ok(usage)
}

fn write_json(path: &Path, value: &Value) -> anyhow::Result<()> {
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
