---
title: "Connect your harness to Submilli"
description: "Configure your harness to use Submilli for code execution."
showDescription: false
slug: how-to/connect-a-harness
sidebar:
  order: 0
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "901c931674ab57f6ea7257930a5c27f073fc4b0aed6e6654c5ba7daebbaa35ac"
  confirmedAt: "2026-10-18T13:09:05.218103+00:00"
# Unpublished AI draft. Human prose rewrite required before publication.
---

Choose your harness. Each guide starts with a prompt for your coding agent to
inspect the project, help prepare any missing setup, and connect Submilli.
Manual instructions follow if you prefer to make the changes yourself.



| Harness how-to | Language | Integration reference |
| --- | --- | --- |
| [Mastra](/docs/how-to/connect-mastra) | TypeScript | [MCP example](https://github.com/submilli/submilli-runtime/tree/main/examples/harnesses/mastra) |
| [LangChain deepagents](/docs/how-to/connect-deepagents) | Python | [Native integration (`langchain-submilli`)](https://github.com/submilli/submilli-python/tree/main/packages/langchain-submilli#readme) |
| [OpenAI Agents SDK](/docs/how-to/connect-openai-agents) | Python | [MCP example](https://github.com/submilli/submilli-runtime/tree/main/examples/harnesses/openai-agents) |
| [Claude Agent SDK](/docs/how-to/connect-claude-agent-sdk) | TypeScript | [MCP example](https://github.com/submilli/submilli-runtime/tree/main/examples/harnesses/claude-agent-sdk) |
| [Vercel AI SDK](/docs/how-to/use-the-http-api) | TypeScript | [HTTP example](https://github.com/submilli/submilli-runtime/tree/main/examples/harnesses/vercel-ai-sdk-http) |

:::note[For manual setup]
You’ll need:

- A [running Submilli server](/docs/server/run-the-server) and a token it accepts.
- A [registered Blueprint](/docs/server/register-a-blueprint), which sets the
  permissions for code sent to Submilli.
:::
