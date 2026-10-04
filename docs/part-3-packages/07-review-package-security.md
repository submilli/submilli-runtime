---
title: "Review a package's security"
description: "How to have a coding agent review a package's authorization: run the review locally with Codex or Claude Code, read and keep its report, require it in CI, and make deployment wait for it."
slug: packages/review-package-security
# The report excerpt is from a Codex 0.160.0 review run on 2026-10-04 with
# the CLI built from main b5002307; the same day, Claude Code 2.1.288 with
# claude-opus-5-5 found the tutorial's flaw locally (exit 1). The Claude Code
# CI setup was verified on GitHub-hosted CI (run 37185669929) with a
# source-built CLI. The CLI also accepts `-a copilot`; this page leaves it out.
sidebar:
  order: 7
---

A blueprint's rules see only what a package passes to `check()`. A package
can check one customer and still return another customer's data, and
neither its tests nor the compiler need notice: the tag and the `check`
agree, and a test that asks for a customer's charges passes when it gets
too many. An agent that reads the package with that question in mind can.

This guide shows you how to review a package's authorization with a coding
agent: run the review locally, read and keep its report, require it in CI
with the agent your team uses, and make deployment wait for it. [Verify a
package in CI](/docs/tutorials/verify-a-package-in-ci#have-an-agent-review-it)
walks through one review end to end with Codex.

## Run a review

From the package project, with the agent installed and signed in:

```sh
submilli build security-review -a codex -m gpt-6.1-sol -e high --fail-on high --output review.json
```

`-a` picks the agent, `-m` its model, and `-e` how hard it reasons.
`--fail-on` is the lowest severity that fails the command, and
`--output` saves the report to a new file; the command refuses to
overwrite one. `-p @acme/billing` limits the review to one package and
the local packages it depends on.

| Agent | `-a` | Install | Sign in locally | Model, for example |
| --- | --- | --- | --- | --- |
| Codex | `codex` | `npm install -g @openai/codex@0.160.0` | `codex login` | `gpt-6.1-sol` |
| Claude Code | `claude` | `npm install -g @anthropic-ai/claude-code@2.1.288` | `claude auth login`, or `ANTHROPIC_API_KEY` set | `claude-opus-5-5` |

The agent runs with its tools turned off and sees only the package's
source, which goes to that agent's model provider under your account's
terms. Install the agents from a pinned version, as here; the review runs
the executable it finds on `PATH`.

## Read the result

The exit code is the verdict:

| Exit | Meaning |
| --- | --- |
| `0` | The review finished, with no finding at or above `--fail-on` |
| `1` | The review finished, with at least one such finding |
| `2` | The review didn't finish: the agent couldn't sign in, timed out, or didn't cover every file |

The findings print with their file, line, evidence, and fix. The report
adds what was reviewed, down to a hash of every file:

```sh
jq '{status, agent, agent_version, model, files, findings}' review.json
```

```json
{
  "status": "complete",
  "agent": "codex",
  "agent_version": "codex-cli 0.160.0",
  "model": "gpt-6.1-sol",
  "files": {
    "capabilities.yaml": "88ac5694770e8ac47c8de59f0f7413ab729c243c856a075a13592cb4338cdb83",
    "docs/readme.md": "cd2a343d7d5b078ebcb806ce7cc2fdd7c740421a00e5d078fda576965e808ef2",
    "src/lib.ts": "1774da90940199f002442c372b0b1ba592da9d434c3ba5edd3d20b9c90a19248",
    "submilli.toml": "489b52f52df72d837b686d3199344d2cf0af1224ef228c4203d53b4cea605b0f",
    "tests/lib.test.ts": "f122f65346cdf5325160697bb9873f5bd3f3f5e195dacae6b96b3592de617dcd"
  },
  "findings": []
}
```

Keep the report with the revision it reviewed: the hashes say exactly
which source the verdict covers. A package that depends on a package
from another repository reviews incomplete, with a coverage gap for the
dependency, and exits `2`; review that dependency in its own project.

## Require it in CI

The `review` job in [Verify a package in
CI](/docs/tutorials/verify-a-package-in-ci#have-an-agent-review-it)
runs on every pull request and push to main, installs the agent and a
pinned Submilli release, reviews the package, and uploads the report
even when the review fails. For Claude Code,
the install, the credential, and the agent and its model change:

| Agent | Install step | The review step's environment |
| --- | --- | --- |
| Codex | `npm install -g @openai/codex@0.160.0` | `CODEX_API_KEY: ${{ secrets.CODEX_API_KEY }}` |
| Claude Code | `npm install -g @anthropic-ai/claude-code@2.1.288` | `CLAUDE_CODE_OAUTH_TOKEN: ${{ secrets.CLAUDE_CODE_OAUTH_TOKEN }}` |

The review step then names the agent and its model:

```yaml title=".github/workflows/test.yml (the review job's steps, for Claude Code)"
      - name: Install Claude Code
        run: npm install -g @anthropic-ai/claude-code@2.1.288
      # … install Submilli, as in the tutorial
      - name: Review authorization
        env:
          CLAUDE_CODE_OAUTH_TOKEN: ${{ secrets.CLAUDE_CODE_OAUTH_TOKEN }}
        run: >-
          submilli build security-review -a claude
          -m claude-opus-5-5 -e high --fail-on high
          --output "$RUNNER_TEMP/security-review-${{ github.run_id }}-${{ github.run_attempt }}.json"
```

Give the credential to the review step only, and require the review job
in the branch's protection rules. Don't add `continue-on-error`: a
review that can't sign in exits `2` and should fail the check, not skip
it.

**Codex** in CI uses an OpenAI API key, billed to the API project, not to
a ChatGPT subscription. Create one under [API
keys](https://platform.openai.com/api-keys) and save it as the
`CODEX_API_KEY` repository secret. A ChatGPT subscription works only on
a private repository's self-hosted runner that keeps Codex's sign-in
between jobs, which run one at a time, and never restore an older
sign-in over the one Codex refreshed; OpenAI excludes public
repositories from it. See [Codex
automation](https://developers.openai.com/codex/noninteractive).

**Claude Code** in CI uses your Claude subscription (Pro, Max, Team, or
Enterprise) through a long-lived token. Create it, after `claude auth
login`, and save it as a repository secret; `gh` prompts for the value,
so it never lands in your shell history:

```sh
claude setup-token
gh secret set CLAUDE_CODE_OAUTH_TOKEN
```

Don't also set `ANTHROPIC_API_KEY` for the step: Claude Code prefers it
and bills the API instead. Renew the token when it expires. See [Claude
authentication](https://code.claude.com/docs/en/authentication#generate-a-long-lived-token).

## Make deployment wait for it

Review the package's own repository, at the revision you deploy. Reviewing
the repository that holds your blueprints doesn't inspect the packages it
installs. When the packages live elsewhere, as in [Manage blueprints in
Git](/docs/tutorials/manage-blueprints-in-git), pin a commit in
`packages.txt` only after its review passed, and keep that review's report
with the deployment.

When the review and the deployment are jobs of one workflow, make the
deployment need both checks, and keep its condition:

```yaml title=".github/workflows/blueprints.yml (fragment)"
  deploy:
    if: github.event_name == 'push'
    needs: [check, review]
```

A merge commit is a different revision from the one the pull request
reviewed; the workflow's push to main reviews it again before the
deployment runs.
