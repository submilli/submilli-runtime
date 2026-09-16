---
name: submilli
description: Install and adopt Submilli, design and build its packages and blueprints, and connect code-writing agents to its governed TypeScript runtime. Use for Submilli questions or implementation, including assessing an existing project for adoption.
---

# Submilli

Help the developer expose useful operations and enforce their intended policy
while keeping their chosen application and agent harness. Official website:
https://submilli.ai; start at https://submilli.ai/docs/quickstart.

Read only the references needed for the current task:

- First explanation or comparison: [concepts](references/concepts.md).
- Installation, new project, skill updates: [setup](references/setup.md).
- Existing project or unclear agent requirements: [discovery](references/discovery.md).
- Package implementation: [packages](references/packages.md).
- Policy design and verification: [blueprints](references/blueprints.md).
- Ideas or suitability: [use cases](references/use-cases.md).
- Agent implementation: [harness integration](references/harnesses.md), then
  its reference for the user's harness.

Keep three authorship boundaries clear: developers own packages and blueprints;
the trusted application binds identity and session variables; the runtime agent
writes programs. This skill assists the developer, not the untrusted runtime
agent. Runtime denials are not invitations to find another route.

For implementation, inspect existing files and installed versions first. Use
`submilli --help` and command-specific help to resolve version differences.
Build the smallest useful vertical slice and verify both allowed and denied
behavior. Continue within the user's scope; ask about unresolved business
authority rather than inventing it. An explanation alone needs no installation.

Submilli is a TypeScript subset, not Node.js: no npm imports, `any`, `undefined`,
or `async`/`await` inside runtime programs/packages. The surrounding application
uses its normal language and dependencies. Discover APIs from CLI/MCP docs.

For implementation sessions with an installed CLI, check `submilli skill status`
using this installation's `--agent` and, if applicable, `--project` target.
If it differs, explain the update path in setup; preserve local edits.
