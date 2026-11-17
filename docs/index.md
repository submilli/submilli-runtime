---
title: Submilli documentation
slug: ""
description: The runtime for the code your agent writes — under rules you set.
hero:
  title: The runtime for the code your agent writes — under rules you set.
  tagline: >-
    Submilli runs the programs your agent writes, under rules you set, checked
    on every call and enforced by the runtime itself.
  actions:
    - text: Start the quickstart
      link: /docs/quickstart
      variant: primary
    - text: Read the introduction
      link: /docs/why
      variant: secondary
nextSteps:
  - why
  - quickstart
  - blueprints
  - tutorials/connect-a-harness
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "090e3b5a7ff750d78e76db223e49ac15d1e33dec7e2e88992d9ccb461f42ef2e"
  confirmedAt: "2026-10-05T13:01:53.009Z"
---

Start with the [introduction](/docs/why) for the argument and a first
look at a Blueprint, then the quickstart to install the CLI and run your first
agent-generated program.

## Benchmarks

The [AppWorld benchmark](https://github.com/submilli/submilli-appworld) compares Submilli with MCP, QuickJS code mode, and tool calling. Each arm ran the 168 tasks in AppWorld's `test_normal` split, with the same models and the same Deep Agents harness.

Submilli beat MCP on all seven model setups, with up to 21 points higher success and up to 63% lower cost. QuickJS code mode had up to 16 points lower success and up to 52% higher cost. Against tool calling with the same APIs, Submilli passed the most tasks, tying tool calling when DeepSeek ran at high effort, and was the cheapest in six of the seven setups. The repository has the tables and the method.
