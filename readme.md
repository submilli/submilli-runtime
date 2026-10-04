![Submilli](docs-site/public/favicon.svg)

# Submilli

A code-execution runtime with semantic security, for business agents that
generate code. "Allow a refund up to $500", not as a safeguard in the prompt,
but as a check outside the model's control.

[Docs](https://submilli.ai/docs/) ·
[Quickstart](https://submilli.ai/docs/quickstart) ·
[Discord](https://discord.gg/VphpukeGGj) ·
[Website](https://submilli.ai)

[![CI](https://github.com/submilli/submilli-runtime/actions/workflows/ci.yml/badge.svg)](https://github.com/submilli/submilli-runtime/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/submilli/submilli-runtime?include_prereleases&label=release)](https://github.com/submilli/submilli-runtime/releases)
[![Container image](https://img.shields.io/badge/container-ghcr.io-blue)](https://github.com/submilli/submilli-runtime/pkgs/container/submilli-runtime)
[![License: Apache 2.0](https://img.shields.io/badge/license-Apache%202.0-blue)](LICENSE)
[![Docs](https://img.shields.io/badge/docs-submilli.ai-informational)](https://submilli.ai/docs/)
[![Discord](https://img.shields.io/discord/1553732232928165958?label=discord)](https://discord.gg/VphpukeGGj)

---

Code mode and programmatic tool calling started the movement toward agents
that write code, instead of calling tools one by one.

Submilli is the runtime for those agents. The agent submits TypeScript code,
and the Submilli runtime executes it in WebAssembly for isolation. We rebuilt
the runtime completely, so there is no `node:http` or `node:fs`. It is a new
runtime, built purposely for agents.

Submilli comes with governance, but from the inside out. Before any call to
the outside world, we first check the environment's permissions (the
blueprint) to see if the call is allowed. And we don't just check the IP,
domain, or port. The package author defines a semantic language for each
operation, and you filter what your agent can do in those terms: "Allow a
refund up to $500, only for customer 123". These rules are outside the
model's control. They are not a prompt.

We also gave the ecosystem a reset. All the packages for Submilli are written
from scratch, purposely for agents, with semantic security. We don't use npm
packages, and while we do support MCP servers, packages are the native way to
work with Submilli.

## What a rule looks like

Blueprints are one of Submilli's main building blocks, together with
packages. A blueprint defines the environment the agent's code runs in. You
write it in YAML.

The permissions block in a blueprint defines what the code can do, and you
fill it by adding capabilities. Package authors publish capabilities, and you
grant them to the agent in the blueprint. You can also define variables for a
blueprint, which is a very strong concept. Now you control not only the
agent's capabilities, but also the context it can use them in. In the example
below, we allow billing operations only for a specific customer, and credits
only up to $500. If the agent tries a different customer, the operation
fails.

```yaml
variables:
  customerId:
    required: true

permissions:
  main:
  - capability: acme.com/charges.list
    filter: customerId == ${vars.customerId}
    action: allow
  - capability: acme.com/credits.apply
    filter: customerId == ${vars.customerId} and amount <= 50000  # cents
    action: allow
```

## Install

macOS and Linux:

```sh
curl -fsSL https://submilli.ai/install.sh | sh
```

Windows (PowerShell):

```powershell
irm https://submilli.ai/install.ps1 | iex
```

This installs the `submilli` CLI and `submilli-server`. To run the server in
production, use [Docker Compose](https://submilli.ai/docs/server/deploy-with-compose),
the [Helm chart](https://submilli.ai/docs/server/deploy-on-kubernetes), or
[systemd](https://submilli.ai/docs/server/deploy-on-linux).

## Connect your agent

Submilli keeps your harness. Your agent gets tools to run programs, over MCP
or HTTP. There are tutorials for
[LangChain Deep Agents](https://submilli.ai/docs/tutorials/connect-deepagents),
[Mastra](https://submilli.ai/docs/tutorials/connect-mastra),
the [OpenAI Agents SDK](https://submilli.ai/docs/tutorials/connect-openai-agents),
the [Claude Agent SDK](https://submilli.ai/docs/tutorials/connect-claude-agent-sdk),
and [plain HTTP](https://submilli.ai/docs/tutorials/use-the-http-api).

## Features

- [Blueprints](https://submilli.ai/docs/blueprints) with rules on an operation's
  arguments, bound per session, and everything denied by default.
- [Packages](https://submilli.ai/docs/packages) that wrap your APIs and hold the
  credentials, so generated code never sees a secret. Curated packages for
  GitHub, Slack, Gmail, Google Drive and Calendar, Linear, Notion, Sentry, and
  web search are [included](https://submilli.ai/docs/reference/curated-packages).
- [Limits](https://submilli.ai/docs/server/set-limits) on fuel, memory, time,
  stack, and model tokens. A failing run ends alone, and the rest of the
  server keeps serving.
- An [audit trail](https://submilli.ai/docs/reference/audit-trail) of every
  refusal, run, session, and admin change.
- MCP servers as packages, with rules on their tools.
- Checks for package authors: `--deny-warnings` in CI and an
  [agent security review](https://submilli.ai/docs/packages/review-package-security).
- HTTPS, API tokens with admin and user roles, and an encrypted secret store.

## Status

Submilli is young and moving quickly. Releases are on the
[releases page](https://github.com/submilli/submilli-runtime/releases).
Breaking changes are called out in the release notes, and a blueprint that
uses a removed feature fails to load with a message that says what to write
instead.

Submilli Enterprise is fault tolerant. To learn more, email
[hello@submilli.ai](mailto:hello@submilli.ai).

## Repository

| Path | What it holds |
| --- | --- |
| `crates/` | The compiler, runtime, CLI, and server, in Rust |
| `packages/` | The curated packages |
| `charts/` | The Helm chart |
| `docs/`, `docs-site/` | The book at [submilli.ai/docs](https://submilli.ai/docs/) |
| `skills/` | The skill that teaches coding assistants to write blueprints and packages |
| `examples/` | The quickstart and harness examples |

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) first. Contributions need the
[CLA](CLA.md). Report security issues as described in
[SECURITY.md](SECURITY.md), not in public issues. Questions are welcome on
[Discord](https://discord.gg/VphpukeGGj).

## License

[Apache License 2.0](LICENSE).
