---
title: "Connect an agent"
description: "Choose a short connection check for your agent library."
slug: tutorials/connect-a-harness
sidebar:
  order: 0
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "464a7984d336e8319b275cb419c8ce8c890b3edd676fafd10ea371ac76741ca2"
  confirmedAt: "2026-10-18T13:09:05.218103+00:00"
# Unpublished AI draft. Human prose rewrite required before publication.
---

These guides check one thing: can your agent library connect to Submilli?

An agent library is the software your application uses to talk to an AI model
and carry out its requests. You may also see it called a *framework* or
*harness*. Submilli is a separate program that runs code sent by the agent.

You will start Submilli on your computer. Then you will connect your chosen
library to it. The check prints `Connected to Submilli.` when the connection
works. It does not ask a model a question or use model credits.

## Choose your library

Each page includes its own setup. Start with the one you use.

| Library | Language for the connection code |
| --- | --- |
| [Mastra](/docs/tutorials/connect-mastra) | TypeScript |
| [LangChain deepagents](/docs/tutorials/connect-deepagents) | Python |
| [OpenAI Agents SDK](/docs/tutorials/connect-openai-agents) | Python |
| [Claude Agent SDK](/docs/tutorials/connect-claude-agent-sdk) | TypeScript |
| [Vercel AI SDK](/docs/tutorials/use-the-http-api) | TypeScript |

SDK stands for software development kit. Here, it is a library you install
to write the connection code.

The first four checks confirm that the library can see Submilli's tool for
running code. A tool is an action the agent can request. The Vercel check
also sends a tiny program through its tool and checks the returned text.
This confirms that the Vercel tool can send code to Submilli and receive
its result.
