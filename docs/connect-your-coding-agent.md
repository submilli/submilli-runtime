---
title: "Connect your coding agent"
description: "Connect Claude Code, Codex, Cursor, or VS Code to a Submilli workspace MCP server so it can read agents, schemas, and sessions, and dry-run changes, as you."
slug: connect-your-coding-agent
docType: how-to
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "d48d59eb558fcd051495249c6031aecd3b8be7234c6bed67e107e4fef5df43ca"
  confirmedAt: "2026-11-13T23:05:53.386+02:00"
---
A Submilli workspace can run an MCP server. MCP (Model Context
Protocol) is the standard way coding agents such as Claude Code, Codex, and
Cursor connect to outside tools. Connect yours, and it can read your
workspace's agents, the schema for each kind of resource, and your session
history, and it can check a change with a dry run before anyone applies it.
It works as you: it can do only what your roles allow, in the spaces you
pick. A space is the boundary Submilli puts around one team, customer, or
use case.

This page is for that workspace server. It is a different endpoint from the
runtime MCP your harness uses to run programs, which lives at
`/mcp/<blueprint>` on `submilli-server`. See [The server](/docs/server#mcp).

On Submilli's current agent server, the address is:

```text
https://agents.i2.submilli.ai/mcp
```

For another workspace, use its agent server address followed by `/mcp`.

## Before you start

- You need a user account on the workspace, the one you use to sign in to the
  console.
- Your client must support MCP over Streamable HTTP with OAuth sign-in. Claude
  Code, Codex, Cursor, and VS Code do.
- Access is by signing in once, interactively, in the client. This server has
  no service-account token, static bearer token, or environment-secret path.
- Sign-in works after an OAuth consent fix is deployed to the workspace. That
  fix is tracked as submilli-agents pull request 55, in the private agent
  server repository. Until it is live, the Allow button can look dead, or the
  browser may not return to your client.
- If the address returns 404, check it for typos. If it is right, MCP is
  turned off on that workspace. Ask the person who runs it to enable MCP.

## Claude Code

```sh
claude mcp add --transport http --scope user submilli_agents https://agents.i2.submilli.ai/mcp
```

Then type `/mcp` in Claude Code, choose **submilli_agents**, and choose
Authenticate. Sign in in the browser window that opens.

## Codex

Add this to `~/.codex/config.toml`, or run
`codex mcp add submilli_agents --url https://agents.i2.submilli.ai/mcp`:

```toml
[mcp_servers.submilli_agents]
url = "https://agents.i2.submilli.ai/mcp"
```

Then sign in:

```sh
codex mcp login submilli_agents
```

If the sign-in page offers only the required read permission, your client did
not ask for the others. Pass `--scopes` with the permissions you want. This
example asks for read, apply (which also covers dry runs), transcripts, and
volumes:

```sh
codex mcp login submilli_agents --scopes resources:read,resources:apply,transcripts:read,volumes:read
```

`resources:delete` is the fifth permission, if you need it.

Codex can also limit which tools it will call, even after you grant broader
permissions on the sign-in page. That is a client setting, not a limit of the
server. To allow only the eight read-only tools that list and read definitions
and sessions, add `enabled_tools` to the same block:

```toml
[mcp_servers.submilli_agents]
url = "https://agents.i2.submilli.ai/mcp"
enabled_tools = [
  "list_resource_kinds",
  "describe_resource_kind",
  "list_resources",
  "get_resource",
  "list_resource_revisions",
  "get_resource_revision",
  "list_sessions",
  "get_session",
]
```

## Cursor

Add this to `~/.cursor/mcp.json`, or to `.cursor/mcp.json` in one project:

```json
{
  "mcpServers": {
    "submilli_agents": { "url": "https://agents.i2.submilli.ai/mcp" }
  }
}
```

Then sign in to **submilli_agents** from Cursor's MCP settings.

## VS Code and other clients

In VS Code, add this to `.vscode/mcp.json`, or use **MCP: Add Server** and
choose HTTP:

```json
{
  "servers": {
    "submilli_agents": { "type": "http", "url": "https://agents.i2.submilli.ai/mcp" }
  }
}
```

Other clients need only the server address. They find the sign-in page on their
own. A program with no browser cannot connect. This server has no token path.

## What you choose when you sign in

The sign-in page shows which application is asking and lets you pick:

- **Spaces.** Pick the spaces the application can see. Global resources, the
  ones shared by every space such as models and packages, are a separate box.
  A space created later needs a new sign-in.
- **Read resource definitions and list sessions.** Always on.
- **Apply resources and dry runs (including Space grants and future agent
  execution).** Off until you check it. One permission covers both dry-run and
  save, so an agent that can dry-run can also save. Tell it to dry-run only,
  and review before anything is applied.
- **Delete resource definitions.** Off until you check it.
- **Read transcripts, tool calls and results.** Off until you check it.
- **Browse and download volume contents.** Off until you check it.

Your roles still apply on every call. If you lose a role, the application
loses it too.

## What it can do

| Tool | What it does | Needs |
|---|---|---|
| `list_resource_kinds` | Lists the kinds of resources, such as Agent, Volume, and Trigger | Read |
| `describe_resource_kind` | Shows the schema and examples for one kind | Read |
| `list_resources` | Lists resources of one kind, with secret values hidden | Read |
| `get_resource` | Reads one resource, or an earlier revision | Read |
| `list_resource_revisions` | Lists a resource's change history | Read |
| `get_resource_revision` | Reads one revision | Read |
| `dry_run_resources` | Checks JSON or YAML resources and shows what would change, without saving | Apply |
| `apply_resources` | Saves JSON or YAML resources. Each one succeeds or fails on its own, so a batch can be saved in part | Apply |
| `delete_resource` | Deletes a resource | Delete |
| `list_sessions` | Lists sessions in a space, filtered by agent, channel, trigger, opener, status, or date | Read |
| `get_session` | Shows a session's status and counters, without its messages | Read |
| `read_transcript` | Reads a session's full transcript, with tool calls and results | Transcripts |
| `list_volume_files` | Lists the files in a volume folder | Volumes |
| `read_volume_file` | Shows a preview of one text file and says if it was cut short | Volumes |
| `download_volume` | Downloads a whole volume as a gzip archive, about 192 KB per call. Continue with the returned cursor within ten minutes | Volumes |

A client sees only the tools its sign-in allows.

## What it cannot do

- Start, message, stop, or delete sessions, or fire triggers. Use the console
  for Run now.
- Change users, groups, service accounts, or role bindings, or delete a space.
- Show a secret's value.

## Ask your agent

Once connected, try:

- "List the agents in the support space and what each one may do."
- "Show me the schema for Trigger, then draft a disabled daily trigger for the
  digest agent and dry-run it. Do not apply it."
- "Why did yesterday's refund session stop? Read its transcript."

## Limits

- An access token lasts 15 minutes and the client renews it on its own. The
  connection ends after 30 days. Sign in again then.
- Signing in again is the only way to change the spaces or permissions you
  picked.
- The server records which tool was called, by whom, and whether it worked. It
  does not record arguments, transcript text, or file contents.

## See or remove a connection

Open **Connected applications** from the account menu in the console. It lists
every application you connected, with its spaces, permissions, and dates.
**Revoke** ends a connection on its next call, including access tokens it
already holds.
