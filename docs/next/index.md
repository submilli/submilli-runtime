---
title: "Draft: the new book"
description: "Working index of the restructured book while it is assembled. Hidden from navigation and from the agent exports."
slug: next
pagefind: false
sidebar:
  hidden: true
---

Pages under `docs/next/` are drafts. They are hidden from the sidebar and from
the agent exports until the cutover. Slugs carry a `next/` prefix that the
cutover removes.

## Part 1 · Start here

1. [Why Submilli](/docs/next/why)
2. [Install](/docs/next/install)
3. [Quickstart](/docs/next/quickstart)
4. [Blueprints](/docs/next/blueprints)
5. [Packages](/docs/next/packages)
6. [The server](/docs/next/server)
7. [Your application](/docs/next/application)

## Part 2 · Blueprints

1. [Start a blueprint](/docs/next/blueprints/start-a-blueprint)
2. [HTTP and credentials](/docs/next/blueprints/http-and-credentials)
3. [Keep files and state](/docs/next/blueprints/keep-files-and-state)
4. [Allow Git](/docs/next/blueprints/allow-git)
5. [Allow model calls](/docs/next/blueprints/allow-model-calls)
6. [Add an MCP server](/docs/next/blueprints/add-an-mcp-server)

## Part 3 · Packages

1. [Start a project](/docs/next/packages/start-a-project)
2. [Export a function](/docs/next/packages/export-a-function)
3. [Document the package](/docs/next/packages/document-the-package)
4. [Add a dependency](/docs/next/packages/add-a-dependency)
5. [Write tests](/docs/next/packages/write-tests)
6. [Publish a package](/docs/next/packages/publish-a-package)

## Part 4 · Server

1. [Run the server](/docs/next/server/run-the-server)
2. [Connect the CLI](/docs/next/server/connect-the-cli)
3. [Register a blueprint](/docs/next/server/register-a-blueprint)
4. [Set limits](/docs/next/server/set-limits) (written for [SUB-1269](https://linear.app/submilli/issue/SUB-1269) and [SUB-1272](https://linear.app/submilli/issue/SUB-1272) as fixed; [SUB-1270](https://linear.app/submilli/issue/SUB-1270) re-measures)
5. [Deploy on Linux](/docs/next/server/deploy-on-linux)
6. [Deploy with Compose](/docs/next/server/deploy-with-compose)
7. [Deploy on Kubernetes](/docs/next/server/deploy-on-kubernetes) (completed after SUB-1265; not run on a cluster)
8. [Install private packages on a server](/docs/next/server/install-private-packages)
9. [Mount a shared volume](/docs/next/server/mount-a-shared-volume)

## Part 5 · Tutorials

Read in order, in three groups. The first two are folders,
`with-your-coding-agent/` and `connect-a-harness/`, whose sidebar labels
the cutover sets in `astro.config.mjs`.

With your coding agent:

1. [Craft a blueprint](/docs/next/tutorials/craft-a-blueprint)
2. [Build a package](/docs/next/tutorials/build-a-package) (re-run after [SUB-1300](https://linear.app/submilli/issue/SUB-1300): live tests in `network` files)

Connect a harness ([the sub-tree's index](/docs/next/tutorials/connect-a-harness) carries the shared setup):

3. [Connect Mastra](/docs/next/tutorials/connect-mastra)
4. [Connect deepagents](/docs/next/tutorials/connect-deepagents)
5. [Connect OpenAI Agents](/docs/next/tutorials/connect-openai-agents)
6. [Connect Claude Agent SDK](/docs/next/tutorials/connect-claude-agent-sdk)
7. [Use the HTTP API](/docs/next/tutorials/use-the-http-api)

Other:

8. [Diagnose a denial](/docs/next/tutorials/diagnose-a-denial)
9. [Verify in CI](/docs/next/tutorials/verify-in-ci)
10. [Manage blueprints in Git](/docs/next/tutorials/manage-blueprints-in-git)
11. [Add the GitHub MCP server](/docs/next/tutorials/add-the-github-mcp-server)

## Part 6 · Reference

Stubs only, created for links from Part 1.

- [Blueprint file](/docs/next/reference/blueprint-file)
- [CLI](/docs/next/reference/cli)
- [Curated packages](/docs/next/reference/curated-packages)
- [Errors and limits](/docs/next/reference/errors-and-limits)
- [Package manifest](/docs/next/reference/package-manifest)
- [Permissions](/docs/next/reference/permissions)
- [Server settings](/docs/next/reference/server-settings)
- [Standard library](/docs/next/reference/standard-library)
- [MCP servers](/docs/next/reference/mcp-servers)
