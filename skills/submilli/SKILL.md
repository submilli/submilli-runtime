---
name: submilli
description: Install and adopt Submilli, design and build its packages and blueprints, and connect code-writing agents to its governed TypeScript runtime. Use for Submilli questions or implementation, including assessing an existing project for adoption.
---

# Submilli

Help the developer expose useful operations and enforce their intended policy
while keeping their chosen application and agent harness. Official website:
https://submilli.ai; start at https://submilli.ai/docs/quickstart.

Before reading any reference, if the `submilli` CLI is installed, run
`submilli skill sync` (no flags). It refreshes this skill to the newest release
and never overwrites local edits. If it reports that `SKILL.md` changed,
re-read this file. If the command is missing or fails, continue with the
installed copy; an explanation alone never needs it.

Read only the references needed for the current task. Each holds mechanics
that general knowledge gets wrong; what goes wrong without it is named.

- First explanation or comparison: [concepts](references/concepts.md);
  without it Submilli gets described as a sandbox.
- Installation, new project, skill updates, deploying the server:
  [setup](references/setup.md); without it the server binary is missed or
  deployed where more than the application can reach it.
- Existing project or unclear agent requirements:
  [discovery](references/discovery.md); without it the interview becomes a
  questionnaire or a silent guess at business limits.
- Choosing operations, check fields, package and blueprint boundaries:
  [capability design](references/capability-design.md); without it checks
  guard fields the model can choose.
- Package implementation: [packages](references/packages.md); without it the
  package is written as Node.js and fails `build`.
- Policy design and verification: [blueprints](references/blueprints.md);
  without it the blueprint lints but the server rejects it.
- Ideas or suitability: [use cases](references/use-cases.md).
- Agent implementation: [harness integration](references/harnesses.md), then
  the user's harness reference; without it the adapter or identity binding is
  wrong and denials are misread.
- After writing or changing a package or blueprint:
  [verification](references/verification.md); without it "verified" means
  "it ran once". Delegate to the `submilli-verifier` subagent when installed;
  otherwise run it yourself as a separate pass. Report its findings.

For topics these references do not cover, fetch https://submilli.ai/llms.txt
and follow the relevant Markdown chapter links. Fetch only the chapters needed
for the task. If the site is unavailable, continue with local references and
CLI/MCP documentation, and identify any documentation gap that affects the answer.

Keep three authorship boundaries clear: developers own packages and blueprints;
the trusted application binds identity and session variables; the runtime agent
writes programs. This skill assists the developer, not the untrusted runtime
agent. Runtime denials are not invitations to find another route.

For implementation, inspect existing files and installed versions first. Use
`submilli --help` and command-specific help to resolve version differences.
Build the smallest useful vertical slice and verify both allowed and denied
behavior. Continue within the user's scope; ask about unresolved business
authority rather than inventing it. Ask with the assistant's blocking question
tool when it has one (`AskUserQuestion` in Claude Code, `request_user_input`
in Codex); otherwise ask in chat with numbered options. When nobody can answer
(a pipeline or unattended run), do not stall: take the narrowest grant that
still completes the stated job, and list each such choice as an explicit
assumption in the deliverable. An explanation alone needs no installation.

Report outcomes, not machinery: say what was built and which allowed and
denied cases were exercised, not which references were loaded.

Submilli is a TypeScript subset, not Node.js: no npm imports, `any`, `undefined`,
or `async`/`await` inside runtime programs/packages. The surrounding application
uses its normal language and dependencies. Discover APIs from CLI/MCP docs.
