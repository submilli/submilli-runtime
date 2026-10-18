---
title: "Connect your harness to Submilli"
description: "Configure your harness to use Submilli's code execution tool."
slug: how-to/connect-a-harness
sidebar:
  order: 0
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "df6fb840ea373f6eb49dab50ccde734b56386b06203eb30b401cccac112008f3"
  confirmedAt: "2026-10-18T13:09:05.218103+00:00"
# Unpublished AI draft. Human prose rewrite required before publication.
---

Connect your harness to Submilli so your agent can run code through it.
A harness is the software that runs your agent and handles its tool calls.

Choose your harness below to add the connection to your project.

## Choose your harness

| Harness | Connection code |
| --- | --- |
| [Mastra](/docs/how-to/connect-mastra) | TypeScript |
| [LangChain deepagents](/docs/how-to/connect-deepagents) | Python |
| [OpenAI Agents SDK](/docs/how-to/connect-openai-agents) | Python |
| [Claude Agent SDK](/docs/how-to/connect-claude-agent-sdk) | TypeScript |
| [Vercel AI SDK](/docs/how-to/use-the-http-api) | TypeScript |

You will need a running Submilli server, a token it accepts, and a registered
Blueprint, which sets the permissions for code sent to Submilli.
If you do not have these yet, [run the server](/docs/server/run-the-server)
and [register a Blueprint](/docs/server/register-a-blueprint).
