# Security review tutorial briefs

Shared purpose: turn a package authorization review into a required GitHub check.
Starting point: a private GitHub repository, Submilli installed, GitHub CLI authenticated,
and access to the chosen coding agent. Each tutorial supplies its own package.
Outcomes: distinguish a compiler check from a semantic review; authenticate a reviewer;
read a finding and its evidence; correct a tenant disclosure; retain a report and gate a merge.
Action: run a local review and install a complete GitHub Actions workflow.
Boundaries: command/report contracts live in the security-review reference; deployment
belongs to Manage blueprints in Git. Provider credentials never reach deployment jobs.
Evidence: focused CLI process tests and real Codex and Claude reviews of the vulnerable
and corrected fixture. GitHub verification uses submilli/test-github-package. Record
verification limits in each page's frontmatter. Never manufacture model output.

## Verification record

Implementation based on `0ef8d6cf59daa712f63fd4f32a3e5a268566b5ff`, left uncommitted.
Six independent clean-code, correctness, and edge-case review rounds completed.
Round one found source omissions beneath hidden/artifact-named directories (P1),
startup environment inheritance during agent version detection (P2), and missing
captured tutorial output (P3). All were fixed. Round two had one P3 correction to
the persistent Codex credential description, checked by the parent after the fix.
Round three reviewed the user's change to hosted Codex with API-key authentication.
Before round four, a parent inspection found a Unix filename collision (P2): a
literal backslash was normalized like a directory separator. Platform-specific
separator normalization and a two-filename regression fixed it. Round four had
one P3 correction to remove duplicate secret-setup instructions; the parent
checked that fix. Round five reviewed the switch to GPT-6.1 Sol and the dedicated
Copilot custom-agent profile after live CI revealed that the default conversational
review added tables and follow-up choices to its JSON. Round six reviewed native
JSONL extraction after terminal rendering corrupted otherwise valid JSON. It
requires one final answer and a successful terminal result, and rejects tool-use
events. The profile removes normal agent tools; explicit exclusions also remove
Copilot's built-in skill and SQL tools. All three round-six reviewers reported no
findings. Strict validation of the extracted report remains unchanged.
No unresolved in-scope code findings remain.
The existing build-driver invariant panic was already tracked in SUB-633; its
inventory records a fix merged in `9640d6c1`, newer than this checkout's local main.

Passed:

- Eight process-boundary CLI tests, including all three adapters, findings thresholds,
  invalid/incomplete results, timeout cleanup, source bounds/symlinks, dependency
  coverage, compiler source discovery, startup environment sanitization, and
  Copilot event framing with malformed/ambiguous/failed-result rejection.
- Workspace clippy with all targets and warnings denied; Rust formatting.
- Documentation checks (eight tests), static-site build, generated CLI reference.
- Dedicated skill frontmatter/body validator; graphify AST update.
- Actual Codex 0.160.0 and Claude Code 2.1.288 reviews: vulnerable fixture exits 1;
  corrected fixture exits 0. Package checks and regression tests pass.

Full suites and conformance were not run during development. Routine Rust HTTP
checks were disabled and package tests used `--skip-network`; the three live agent
reviews separately exercised their provider connections. No compiler/runtime
language behavior changed.

GitHub verification builds an unreleased source snapshot on an isolated test branch,
then checks the deliberately flawed fixture and its correction. This differs from
the tutorials' future pinned release installation. The release installation and
required-check settings still need verification when the command is released.


Live CI evidence:

- [Run 37185669929](https://github.com/submilli/test-github-package/actions/runs/37185669929):
  Linux source build and Claude passed. Claude found the cross-customer leak, then
  returned a clean corrected report; both artifacts were inspected. Copilot failed
  closed with an incomplete report, retained as an artifact. The unassigned
  self-hosted Codex job was canceled after the user chose GitHub-hosted/API-key CI.
- [Copilot probe 37186458101](https://github.com/submilli/test-github-package/actions/runs/37186458101):
  the direct no-tool CLI request independently returned “Access denied by policy
  settings.” The user subsequently enabled organization policy; this historical
  failure is superseded by the later authenticated runs.
- [Codex GPT-6.1 Sol run 37187271341](https://github.com/submilli/test-github-package/actions/runs/37187271341):
  GitHub-hosted runner with `CODEX_API_KEY`, vulnerable exit 1 and corrected exit 0;
  both downloaded reports validate the selected model and expected findings.
  Local Codex with GPT-6.1 Sol also passed both cases.
- Copilot reached GPT-6.1 Sol after the policy change, but its default review
  formatting produced extra Markdown after JSON. Runs 37187210643 and 37187411798
  correctly returned incomplete rather than accepting ambiguous output. The
  dedicated profile alone still encountered terminal-rendering corruption in
  37187866158. Native JSONL capture in 37188181846 confirmed valid final-answer
  content and exposed always-available skill/SQL tools. The JSONL parser and
  explicit tool exclusions pass eight focused process tests.
- [Copilot GPT-6.1 Sol run 37188559097](https://github.com/submilli/test-github-package/actions/runs/37188559097):
  Linux source build and both reviews passed with Copilot CLI 1.0.91. The
  downloaded reports show one high-severity cross-customer disclosure before
  the correction and no findings afterward, complete file coverage, no gaps,
  and no errors. The tutorial includes these real result excerpts. Local
  personal-account access remains unavailable; workflow-token access is verified.
- [Copilot isolation run 37188863824](https://github.com/submilli/test-github-package/actions/runs/37188863824):
  both review outcomes passed again, and native model metadata confirmed zero
  available tools after the explicit exclusions.

All three published tutorial workflows select GitHub-hosted runners. No
implementation commit or PR was created.
