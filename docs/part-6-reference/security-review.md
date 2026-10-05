---
title: Security review
description: "Agent selection, authentication, review scope, report fields, limits, and exit codes for package security reviews."
slug: reference/security-review
# Agent versions used during implementation: Codex 0.160.0, Claude Code
# 2.1.288, Copilot CLI 1.0.91.
sidebar:
  order: 12
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "59de2570557a5e96b2691d1cea95e0868bbfa74d37f3e723d7023131e9afbfd2"
  confirmedAt: "2026-10-05T10:59:51.478Z"
---

`submilli build security-review` runs an installed coding agent against a source
snapshot of a package project. It reviews whether package authorization checks
protect the operations and data exposed to callers. It does not certify a
package, execute tests, or change runtime enforcement. Model reviews can miss
defects and report false positives.

## Invocation

```sh
submilli build security-review -a codex -m gpt-6.1-sol -e high -p @acme/billing --fail-on high --output review.json
```

The [CLI reference](/docs/reference/cli#submilli-build-security-review)
lists the options. Beyond their help text:

| Option | Behavior |
| --- | --- |
| `-a`, `--agent` | Runs the `codex`, `claude`, or `copilot` executable found on `PATH` |
| `-m`, `--model` | Passed to the agent as given, except `astra`, which becomes `gpt-6-astra` for Codex and Copilot and is refused for Claude |
| `-e`, `--effort` | Passed to the agent. Whether the model honors it is up to the provider |
| `--output` | Never replaces an existing file. The file is created, marked incomplete, before the review begins |

The review procedure is built into the CLI. Nothing is installed or
downloaded for it. Which models an account can use, and what a review
costs, is up to the provider.

## Scope and execution

The command finds `submilli.toml` by walking upward from the current directory.
It includes the manifest, `.ts` and `.subm` files, package readmes,
capability YAML, and package lockfiles under the selected package directories.
Every `.ts` and `.subm` file beneath a package's `src` directory is included.
Outside `src`, it skips hidden entries, `node_modules`, `target`, `dist`, and
`graphify-out`.
Imports requiring omitted source must be reported as coverage gaps.

Local package dependencies are included. External package source is not fetched.
Each declared external dependency produces a coverage gap, so that review exits
2 even if the agent finds no defect. Review such dependencies in their source
projects. Standard library operations are covered by the bundled review procedure.

Source symlinks and nonregular source files are rejected. Input is bounded to
512 KiB of file content, 1024 files, 20,000 directory entries, and 64 directory
levels. Exceeding a limit fails without truncating the review. Use `-p` to reduce
the selected scope. Individual models can have lower usable context limits.
An agent that cannot inspect the full snapshot must report an incomplete review.

The agent receives the snapshot through standard input from a temporary working
directory. Package code and repository hooks are not executed. The adapters
disable agent tools, repository instructions, and extension discovery where
supported. Copilot uses a temporary custom-agent profile with an empty tool list,
explicitly excludes its built-in skill and SQL tools, and reads the report from
its JSON event stream. A missing or failed terminal result, multiple final
answers, or tool-use events make the review incomplete. Codex also runs with its
read-only sandbox.
Beyond these
settings, Submilli doesn't confine the agent executable. It runs as
installed. Source is sent to the
chosen model provider under that account's terms.

Agent output is bounded to 4 MiB. Timeout or interruption stops the child process.
On Unix it also terminates the child's process group. Temporary review files are
removed after cleanup. Raw agent stderr is not copied into reports because it
can contain credentials or private configuration, so an authentication
failure appears only as an incomplete review.

## Authentication

Submilli does not create accounts, mint tokens, or manage subscription refresh.

| Agent | Supported setup |
| --- | --- |
| Codex | Existing Codex login or `CODEX_API_KEY` for `codex exec`. User configuration is ignored, but the existing authentication store is retained, including normal token refresh. |
| Claude Code | Existing subscription login, `CLAUDE_CODE_OAUTH_TOKEN`, or `ANTHROPIC_API_KEY`. User/project settings are ignored. `--bare` is not used because it disables subscription OAuth. |
| Copilot | `COPILOT_GITHUB_TOKEN`, `GH_TOKEN`, or `GITHUB_TOKEN`, or GitHub CLI authentication. A temporary Copilot configuration directory is used, and personal Copilot login/configuration files are not loaded. |

With both `CLAUDE_CODE_OAUTH_TOKEN` and `ANTHROPIC_API_KEY` set, Claude Code
uses the API key. Copilot with `GITHUB_TOKEN` needs the workflow permission
`copilot-requests: write`. [Review a package's
security](/docs/packages/review-package-security#require-it-in-ci) sets up
each agent in CI.

## JSON report

| Field | Meaning |
| --- | --- |
| `schema_version` | Report contract version, currently 1. |
| `status` | `complete` or `incomplete`. Findings do not by themselves make a review incomplete. |
| `submilli_version` | CLI version that orchestrated the review. |
| `agent`, `agent_version` | Selected agent and its reported CLI version, when available. |
| `model`, `effort` | Requested model after Submilli alias expansion and requested effort. This is not attestation of the provider's actual routing. |
| `skill_sha256` | Hash of the embedded review procedure. |
| `packages` | Selected packages, including local dependencies. |
| `files` | Snapshot paths mapped to SHA-256 hashes of the exact supplied contents. |
| `reviewed_files` | Paths the agent reports inspecting. The CLI rejects unknown or duplicate paths and requires all snapshot files for completion. |
| `coverage_gaps` | Missing source or unresolved review coverage. Any gap makes the result incomplete. |
| `findings` | Findings with severity, title, path, one-based line, evidence, and recommendation. Paths and lines are checked against the snapshot. |
| `error` | Execution or report-validation failure, or `null`. |

The CLI validates the report's structure and source references. It cannot
verify that the model reasoned about each file it lists. File hashes identify
content, including local edits, not a Git revision.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Complete review with no finding at or above `--fail-on`. Lower-severity findings may remain. |
| 1 | Complete review with at least one finding meeting the threshold. |
| 2 | Incomplete review, invalid arguments, missing agent, authentication/model failure, timeout, missing source, invalid output, or report-file failure. |

An agent process exiting 0 is insufficient. Its report must validate and cover
every supplied file. Existing report files are refused, so a failed attempt never
reuses an earlier clean report. If the output destination itself cannot be
created or written, the command fails and a usable report may be absent.

[Review a package's security](/docs/packages/review-package-security) runs
the review locally and in CI. [Verify a package in
CI](/docs/tutorials/verify-a-package-in-ci#have-an-agent-review-it) walks
through one review.
