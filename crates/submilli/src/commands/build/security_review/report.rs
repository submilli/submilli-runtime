use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, bail};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use super::{Args, Effort, SKILL, agent::Agent, snapshot::Snapshot};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub(super) enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Finding {
    severity: Severity,
    title: String,
    path: String,
    line: usize,
    evidence: String,
    recommendation: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Response {
    pub complete: bool,
    pub reviewed_files: Vec<String>,
    pub coverage_gaps: Vec<String>,
    pub findings: Vec<Finding>,
}

#[derive(Serialize)]
pub(super) struct Report {
    schema_version: u32,
    status: &'static str,
    submilli_version: &'static str,
    agent: Agent,
    pub agent_version: Option<String>,
    model: String,
    effort: Option<Effort>,
    skill_sha256: String,
    packages: Vec<String>,
    files: BTreeMap<String, String>,
    reviewed_files: Vec<String>,
    coverage_gaps: Vec<String>,
    findings: Vec<Finding>,
    error: Option<String>,
}

impl Report {
    pub fn pending(args: &Args) -> Self {
        Self {
            schema_version: 1,
            status: "incomplete",
            submilli_version: env!("CARGO_PKG_VERSION"),
            agent: args.agent,
            agent_version: None,
            model: args.agent.model(&args.model).to_owned(),
            effort: args.effort,
            skill_sha256: format!("{:x}", Sha256::digest(SKILL)),
            packages: Vec::new(),
            files: BTreeMap::new(),
            reviewed_files: Vec::new(),
            coverage_gaps: Vec::new(),
            findings: Vec::new(),
            error: Some("review has not completed".to_owned()),
        }
    }

    pub fn set_snapshot(&mut self, snapshot: &Snapshot) {
        self.packages.clone_from(&snapshot.packages);
        self.files = snapshot
            .files
            .iter()
            .map(|(name, source)| (name.clone(), source.sha256.clone()))
            .collect();
        self.coverage_gaps.clone_from(&snapshot.coverage_gaps);
    }

    pub fn accept(&mut self, response: Response, snapshot: &Snapshot) -> anyhow::Result<()> {
        let reviewed: BTreeSet<_> = response.reviewed_files.iter().collect();
        if reviewed.len() != response.reviewed_files.len()
            || reviewed
                .iter()
                .any(|name| !snapshot.files.contains_key(*name))
        {
            bail!("agent returned duplicate or unknown reviewed source paths");
        }
        for finding in &response.findings {
            let source = snapshot
                .files
                .get(&finding.path)
                .context("agent finding refers to a path outside the snapshot")?;
            if finding.line == 0 || finding.line > source.content.lines().count() {
                bail!("agent finding has an invalid source line");
            }
            if [&finding.title, &finding.evidence, &finding.recommendation]
                .iter()
                .any(|s| s.trim().is_empty())
            {
                bail!("agent finding is missing its title, evidence, or recommendation");
            }
        }
        if reviewed.len() != snapshot.files.len() {
            self.coverage_gaps
                .push("agent did not review every supplied file".to_owned());
        }
        if !response.complete {
            self.coverage_gaps
                .push("agent reported an incomplete review".to_owned());
        }
        self.reviewed_files = response.reviewed_files;
        self.coverage_gaps.extend(response.coverage_gaps);
        self.findings = response.findings;
        self.error = None;
        self.status = if self.coverage_gaps.is_empty() {
            "complete"
        } else {
            "incomplete"
        };
        Ok(())
    }

    pub fn fail(&mut self, error: String) {
        self.status = "incomplete";
        self.error = Some(error);
    }

    pub fn exit_code(&self, threshold: Severity) -> u8 {
        if self.status != "complete" {
            2
        } else if self.findings.iter().any(|f| f.severity >= threshold) {
            1
        } else {
            0
        }
    }

    pub fn print(&self) {
        println!(
            "Security review: {} ({} finding(s))",
            self.status,
            self.findings.len()
        );
        for finding in &self.findings {
            println!(
                "{:?} {}:{} — {}",
                finding.severity,
                safe(&finding.path),
                finding.line,
                safe(&finding.title)
            );
            println!(
                "  {}\n  Fix: {}",
                safe(&finding.evidence),
                safe(&finding.recommendation)
            );
        }
        for gap in &self.coverage_gaps {
            println!("Coverage gap: {}", safe(gap));
        }
        if let Some(error) = &self.error {
            eprintln!("Review error: {}", safe(error));
        }
    }
}

fn safe(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() {
                c.escape_debug().to_string()
            } else {
                c.to_string()
            }
        })
        .collect()
}

pub(super) fn schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object", "additionalProperties": false,
        "required": ["complete", "reviewed_files", "coverage_gaps", "findings"],
        "properties": {
            "complete": {"type": "boolean"},
            "reviewed_files": {"type": "array", "items": {"type": "string"}},
            "coverage_gaps": {"type": "array", "items": {"type": "string"}},
            "findings": {"type": "array", "items": {
                "type": "object", "additionalProperties": false,
                "required": ["severity", "title", "path", "line", "evidence", "recommendation"],
                "properties": {
                    "severity": {"type": "string", "enum": ["low", "medium", "high", "critical"]},
                    "title": {"type": "string"}, "path": {"type": "string"},
                    "line": {"type": "integer", "minimum": 1},
                    "evidence": {"type": "string"}, "recommendation": {"type": "string"}
                }
            }}
        }
    })
}
