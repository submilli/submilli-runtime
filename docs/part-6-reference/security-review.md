---
title: Security review
description: "Agent selection, authentication, review scope, report fields, limits, and exit codes for package security reviews."
slug: reference/security-review
sidebar:
  order: 32
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

| Option | Meaning |
| --- | --- |
| `-a`, `--agent` | Required: `codex`, `claude`, or `copilot`. The executable must be on `PATH`. |
| `-m`, `--model` | Required provider model identifier. `astra` expands to `gpt-6-astra` for Codex and Copilot; Claude rejects that alias. |
| `-e`, `--effort` | Optional `low`, `medium`, or `high`. Passed to the selected CLI; model support is provider-dependent. |
| `-p`, `--package` | One declared package and its local dependency closure. Omit to review all declared packages. |
| `--fail-on` | Minimum severity that fails a complete review: `low`, `medium`, `high` (default), or `critical`. |
| `--output` | New JSON report file. An existing file is never overwritten. An incomplete report is reserved before review begins. |
| `--timeout` | Agent runtime limit in seconds, from 1 to 3600; default 600. |

The dedicated security-review skill is embedded in the CLI. No skill installation
or prompt download is required. Agent versions used during implementation:
Codex 0.160.0, Claude Code 2.1.288, and Copilot CLI 1.0.91. Provider availability,
model access, and subscription allowances are independent of Submilli.

## Scope and execution

The command finds `submilli.toml` by walking upward from the current directory.
It includes the manifest, TypeScript and legacy `.subm` files, package readmes,
capability YAML, and package lockfiles under the selected package directories.
Every `.ts` and `.subm` file beneath a package's `src` directory is included.
Outside `src`, it skips hidden entries, `node_modules`, `target`, `dist`, and
`graphify-out`.
Imports requiring omitted source must be reported as coverage gaps.

Local package dependencies are included. External package source is not fetched:
each declared external dependency produces a coverage gap, so that review exits
2 even if the agent finds no defect. Review such dependencies in their source
projects. Standard library operations are covered by the bundled review procedure.

Source symlinks and nonregular source files are rejected. Input is bounded to
512 KiB of file content, 1024 files, 20,000 directory entries, and 64 directory
levels. Exceeding a limit fails without truncating the review. Use `-p` to reduce
the selected scope. Individual models can have lower usable context limits;
an agent that cannot inspect the full snapshot must report an incomplete review.

The agent receives the snapshot through standard input from a temporary working
directory. Package code and repository hooks are not executed. The adapters
disable agent tools, repository instructions, and extension discovery where
supported. Copilot uses a temporary custom-agent profile with an empty tool list,
explicitly excludes its built-in skill and SQL tools, and reads the report from
its JSON event stream. A missing or failed terminal result, multiple final
answers, or tool-use events make the review incomplete. Codex also runs with its
read-only sandbox.
This is a static model
review, not a sandbox for running an untrusted agent executable. Use trusted,
pinned agent installations. Source is sent to the chosen model provider under
that account's terms.

Agent output is bounded to 4 MiB. Timeout or interruption stops the child process;
on Unix it also terminates the child's process group. Temporary review files are
removed after cleanup. Raw agent stderr is not copied into reports because it
can contain credentials or private configuration. Diagnose authentication with
the selected CLI separately.

## Authentication

Submilli does not create accounts, mint tokens, or manage subscription refresh.

| Agent | Supported setup |
| --- | --- |
| Codex | Existing Codex login or `CODEX_API_KEY` for `codex exec`. User configuration is ignored, but the existing authentication store is retained, including normal token refresh. |
| Claude Code | Existing subscription login, `CLAUDE_CODE_OAUTH_TOKEN`, or `ANTHROPIC_API_KEY`. User/project settings are ignored. `--bare` is not used because it disables subscription OAuth. |
| Copilot | `COPILOT_GITHUB_TOKEN`, `GH_TOKEN`, or `GITHUB_TOKEN`, or GitHub CLI authentication. A temporary Copilot configuration directory is used; personal Copilot login/configuration files are not loaded. |

For Codex on a trusted private persistent runner, sign in once and retain the
refreshed authentication store between serialized jobs. Do not restore an old
`auth.json` over refreshed credentials. OpenAI excludes public repositories from
this subscription CI pattern. For hosted or public CI, configure an OpenAI API
key as a GitHub secret and expose `CODEX_API_KEY` only to the review step; API
usage is billed separately. See [Codex automation authentication](https://learn.chatgpt.com/docs/non-interactive-mode#authenticate-in-automation).

Claude's `claude setup-token` generates a subscription OAuth token for CI. An
API key takes precedence when both are present. See
[Claude authentication](https://code.claude.com/docs/en/authentication).

Copilot's `GITHUB_TOKEN` integration requires `copilot-requests: write` and the
appropriate billing/model policy. See
[Copilot in Actions](https://docs.github.com/en/copilot/how-tos/copilot-cli/use-copilot-cli-in-actions).

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

The CLI validates the report's structure and source references. It cannot verify
that the model actually reasoned about each file. A complete status describes
reported coverage; it does not establish absence of vulnerabilities. File hashes
identify content, including local edits, rather than asserting a Git revision.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Complete review with no finding at or above `--fail-on`. Lower-severity findings may remain. |
| 1 | Complete review with at least one finding meeting the threshold. |
| 2 | Incomplete review, invalid arguments, missing agent, authentication/model failure, timeout, missing source, invalid output, or report-file failure. |

An agent process exiting 0 is insufficient: its report must validate and cover
every supplied file. Existing report files are refused; a failed attempt never
reuses an earlier clean report. If the output destination itself cannot be
created or written, the command fails and a usable report may be absent.

## Tutorials

- [Codex API key on a GitHub-hosted runner](/docs/tutorials/security-review-codex)
- [Claude Code subscription in GitHub Actions](/docs/tutorials/security-review-claude)
- [GitHub Copilot with the workflow token](/docs/tutorials/security-review-copilot)
