---
title: Why agents execute code — transcript
description: The complete narration of the code-execution introduction.
slug: videos/code-execution-introduction
prev: false
next: false
---

[Execution model](/docs/concepts/execution-model/) is this film’s home. The video
is 1 minute 31 seconds (90.688 seconds), in English. Publication is pending; this
page contains the complete narration of the approved introduction.

The failed-payment workflow is illustrative. The reported research results at
the end concern different tasks and are not a benchmark of Submilli.

## 0:00 — Investigating payment failures

Consider an agent used by a small business. The agent is asked to summarize yesterday's failed payments.

## 0:07 — Tool calls, context and inference

The agent uses external tools such as APIs or MCPs to fetch failed payments, identify the affected accounts, and retrieve relevant support tickets. Each result returns to the model before it can choose the next request. That adds inference work and makes the next request wait.

## 0:27 — Managing model rounds

In this illustrative sequence, three requests and a final interpretation take four model rounds.

## 0:35 — Coordinating with code

Instead of sequential tool calling, the model may write a program that calls the three tools. Inside the program, it totals the failed payments, identifies the impacted accounts, and links them to three support tickets - without another model round.

## 0:51 — Delivering clear insights

The program returns the final report. The model provides the requested summary, financial impact, and customer list while keeping raw records separate from the primary output.

## 1:03 — Efficiency comparison

The process is reduced from four model rounds to two. Routine work runs as code, maintaining a smaller, more focused conversation context.

## 1:14 — Code execution in the agent stack

Anthropic demonstrated lower context use. CodeAct improved task success. Cloudflare built a compact interface to thousands of API endpoints. These results help explain why code execution is becoming part of the agent stack.

## Evidence mentioned in the film

- [Anthropic: code execution with MCP](https://www.anthropic.com/engineering/code-execution-with-mcp)
- [CodeAct paper](https://arxiv.org/abs/2402.01030)
- [Cloudflare: Code Mode for its API](https://blog.cloudflare.com/code-mode-mcp/)
