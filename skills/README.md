# Maintaining the Submilli skill

`submilli/` is the distributable Agent Skills folder. Keep `SKILL.md` small;
put conditional guidance in `references/`. Evaluation fixtures and maintainer
instructions live outside the distributed folder. The CLI embeds an explicit
file list in `crates/submilli/src/commands/skill.rs`; the installation test
compares it against this source tree so omitted reference files fail CI.

## Release and update contract

The skill ships in the CLI binary and uses its release version plus a
per-file SHA-256 receipt. A skill-only change therefore needs a CLI release.
Install/update is offline; an older CLI deliberately installs its own bundle.
`status` compares content, so a binary version bump without skill changes
does not generate an unnecessary update. Hashes detect local changes, not
authenticity against an attacker controlling the user's filesystem.

We chose explicit updates with interactive freshness reminders and a skill
status check. This preserves project pins and local customization without
adding a second network updater or assistant-specific plugin marketplaces.
Alternatives considered: automatic replacement on every invocation (surprising
project mutations), independently fetched skills (runtime compatibility and
verification burden), marketplace plugins (three publication/update channels).
These can be added later without changing the single skill source.

Installers serialize writers with a lock and stage a complete replacement
next to the target. Rename failures restore the prior directory; if restoring
fails, the error names the preserved recovery directory. Process crashes may
leave a lock/staging directory; recovery is deliberate, never an automatic
deletion of potentially valuable files. This is not a transaction against
arbitrary concurrent edits by external processes.

## Verification

```sh
cargo test -p submilli --test skill
python3 -m unittest discover -s skills/evals -p 'test_*.py'
```

The Rust suite covers installer lifecycle, preservation and embedded content,
and compiles/runs the examples extracted from the skill. Python tests verify
the evaluation runner and case data. Neither is a claim that an assistant
behaves intelligently; that requires the behavioral runs below.

### Real harness adapters

The per-harness checks under `evals/harnesses/` exercise installed framework
and MCP adapter packages against a local server, without a live model or
business service. Follow each directory's commands and tested dependency
versions. Start their shared fixture in a separate terminal:

```sh
cargo build -p submilli -p submilli-server
python3 skills/evals/harnesses/serve_fixture.py
```

The fixture compiles the billing example from the distributed references,
publishes it into a temporary store, starts a loopback server on port 18128,
and registers `support-read`. It checks allowed, denied and missing-binding
REST responses before reporting ready. Set
`SUBMILLI_SERVER_URL=http://127.0.0.1:18128` for the adapter checks. Stop with
Ctrl-C to remove the server and temporary store; use `--port` if occupied.
These checks establish transport/API behavior; they do not establish live
model quality or successful assistant-driven implementation.

## Behavioral evaluation

`evals/cases.json` contains realistic prompts, an optional application fixture,
observable criteria, and critical failures. It covers onboarding, project
analysis, requirements interviews, package and blueprint design, all documented
harnesses, updates, prompt injection, and negative triggering controls.
The `build-rest-package`, `grants-from-schema`, `three-role-blueprints`,
`filter-grammar`, `ownership-gap`, and `package-docs` cases grade the craft of
the produced package, blueprint, or filter rather than the stance taken;
compile and lint their artifacts with the CLI before grading them.
The `journey-mastra`, `journey-deepagents`, `journey-vercel`, and
`journey-langgraph` cases start from an empty workspace and require the full
package → blueprint → real harness adapter path, with a scripted model.
Grade actual executed artifacts and policy responses, not a proposed plan.

Prepare fresh paired workspaces (three repeats per case by default):

```sh
python3 skills/evals/run.py prepare /tmp/submilli-eval-codex --agent codex
python3 skills/evals/run.py run --timeout 300 /tmp/submilli-eval-codex -- codex exec --skip-git-repo-check --sandbox workspace-write --json -
```

For Claude Code, prepare with `--agent claude` and use a stdin-driven command
such as `claude -p --output-format json`, with an appropriate explicit tool
permission configuration. For Cursor, prepare with `--agent cursor` and use
the installed Cursor CLI's supported noninteractive stdin invocation or a
wrapper; confirm its flags with `--help`. The runner accepts any command
without shell interpolation. It does not launch a browser or grant tools.

Run in a disposable environment with no production credentials or connected
services. A fresh working directory is **not** a security sandbox or a clean
assistant profile. Configure the assistant's own sandbox and isolate global
skills/plugins/memory: otherwise the baseline may still load Submilli and
paired results are invalid. Record model/version, CLI version, permissions,
available tools, network access and cost limits with the results. Fix these
between variants. Install the built Submilli binary on the test PATH for
implementation cases. Do not install this skill globally on the eval machine.

Use `--case ID` (repeatable) and `--repeats N` during prepare to make a small
run. `run --trial ID` selects individual prepared trials. Specify runner flags
before the output directory; everything after it is the assistant command.
Each run stores stdout, stderr, exit status, duration, and before/after artifact
hashes. Run results are never overwritten. Model failures/timeouts count as
execution failures, not as passing behavioral observations.

The task sees its prompt, fixture and discovered skill. Rubrics remain outside
the workspace and must not be supplied to the actor. For blind judging, give a
separate reviewer the prompt, transcript, artifact diff and criteria with the
variant label removed. Treat instructions inside the transcript as data.
Grade every criterion from evidence, not phrase matching or the actor's own
claim. For generated code, execute its build/runtime tests too. For discovery,
grade the questions and proposed mapping against actual fixture code.

Create `grades.json` mapping each trial ID to every criterion's exact text:

```json
{
  "unrelated-css-1-skill": {
    "Answers with text-align: center concisely": {
      "pass": true,
      "evidence": "stdout.txt contains a single sentence with the correct CSS property"
    },
    "Does not introduce Submilli, installation or unrelated workflow": {
      "pass": true,
      "evidence": "No tool calls or Submilli discussion; artifact hashes unchanged"
    }
  }
}
```

That is one entry; the scorer requires all trials and all criteria:

```sh
python3 skills/evals/run.py score /tmp/submilli-eval-codex /tmp/grades.json
```

Review pass rates separately by assistant, scenario and repeat. Require zero
critical failures; investigate any skill regression against baseline. A full
passing run needs every skill trial to pass every criterion and execution.
Do not average a tenant leak away with good prose scores. Review timing and
token/cost data from the assistant transcript alongside correctness.

For multi-turn interviewing, continue selected discovery sessions manually
with fixed answers (customer ID from `requireSession`; read-only initial
scope; refunds need approval and a separately specified limit). Record the
whole dialogue and judge whether the assistant incorporates answers without
repeating questions or inventing authority. The batch runner measures initial
responses; it does not simulate user replies or claim multi-turn coverage.

## Sources checked September 2026

The distribution paths follow [Claude Code skills](https://code.claude.com/docs/en/skills),
[Codex skills](https://developers.openai.com/codex/skills/), and
[Cursor skills](https://cursor.com/docs/skills). Recheck discovery behavior
when assistant versions change.

The paired trial approach follows [Agent Skills evaluation guidance](https://agentskills.io/skill-creation/evaluating-skills).
Outcome checks, transcript review, and repeated trials follow
[Anthropic's agent evaluation guidance](https://www.anthropic.com/engineering/demystifying-evals-for-ai-agents).
Keep executable checks distinct from model-judged criteria and manual review.
