//! Apply blueprint YAML documents verbatim to the runtime server, which owns
//! schema validation and diagnostics.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::json;

use crate::commands::http;
use documents::ProbedDoc;

mod documents;

#[derive(clap::Args)]
pub struct Args {
    /// YAML file (multi-document ok) or directory of .yaml files to apply.
    #[arg(short, long)]
    file: PathBuf,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    match run(&args) {
        Ok(()) => Ok(ExitCode::SUCCESS),
        Err(e) => {
            eprintln!("error: {e:#}");
            Ok(ExitCode::from(1))
        }
    }
}

fn run(args: &Args) -> Result<()> {
    let mut docs = Vec::new();
    for file in collect_files(&args.file)? {
        let raw =
            fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))?;
        for (i, text) in documents::split_documents(&raw).into_iter().enumerate() {
            docs.push(documents::probe(&file, i + 1, text)?);
        }
    }
    if docs.is_empty() {
        bail!("no YAML documents found in {}", args.file.display());
    }
    let target = BlueprintTarget::from_env()?;
    for doc in &docs {
        apply_blueprint(&target, doc)?;
    }
    Ok(())
}

fn collect_files(path: &Path) -> Result<Vec<PathBuf>> {
    let meta = fs::metadata(path).with_context(|| format!("reading {}", path.display()))?;
    if meta.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    let mut files: Vec<PathBuf> = fs::read_dir(path)
        .with_context(|| format!("reading {}", path.display()))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .is_some_and(|ext| ext == "yaml" || ext == "yml")
        })
        .collect();
    files.sort();
    if files.is_empty() {
        bail!("no .yaml files in {}", path.display());
    }
    Ok(files)
}

struct BlueprintTarget {
    base: String,
    client: ureq::Agent,
}

impl BlueprintTarget {
    fn from_env() -> Result<Self> {
        match std::env::var("SUBMILLI_SERVER_URL") {
            Ok(v) if !v.trim().is_empty() => Ok(BlueprintTarget {
                base: v.trim_end_matches('/').to_string(),
                client: http::client(),
            }),
            _ => bail!("SUBMILLI_SERVER_URL is not set; export the submilli-server's base URL"),
        }
    }
}

#[derive(Deserialize)]
struct AppliedBlueprint {
    name: String,
    created: bool,
}

fn apply_blueprint(target: &BlueprintTarget, doc: &ProbedDoc) -> Result<()> {
    let mut url = url::Url::parse(&target.base)
        .with_context(|| format!("SUBMILLI_SERVER_URL is not a valid URL: {}", target.base))?;
    url.path_segments_mut()
        .map_err(|()| anyhow::anyhow!("SUBMILLI_SERVER_URL cannot be a base URL"))?
        .extend(["v1", "blueprints", &doc.name]);
    let resp = target
        .client
        .put(url.as_str())
        .send_json(json!({ "yaml": doc.text }))
        .with_context(|| {
            format!(
                "cannot reach the submilli-server at {} (SUBMILLI_SERVER_URL)",
                target.base
            )
        })?;
    let status = resp.status().as_u16();
    if status != 200 {
        let message = resp
            .into_body()
            .read_json::<http::ServerError>()
            .map_or_else(|_| format!("server returned HTTP {status}"), |e| e.message);
        bail!(
            "submilli-server rejected {} (document {}): {message}",
            doc.file.display(),
            doc.index
        );
    }
    let applied: AppliedBlueprint = resp
        .into_body()
        .read_json()
        .context("submilli-server returned malformed JSON")?;
    let verb = if applied.created { "Added" } else { "Updated" };
    println!("{verb} blueprint '{}'", applied.name);
    Ok(())
}
