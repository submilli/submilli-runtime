---
title: "Add the GitHub MCP server"
description: "Give an agent GitHub through GitHub's hosted MCP server: declare it in a blueprint, register an OAuth application and log in once, allow a tool and call it, run it on a server, and know where a per-user token belongs instead."
slug: tutorials/add-the-github-mcp-server
sidebar:
  order: 11
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "07a4ce4718ed88cfe591b56af7fea7b983a9ff0f9f1686f21b78733b28023c0e"
  confirmedAt: "2026-10-05T10:59:51.481Z"
---

The curated `@submilli/github` package covers what most agents need from
GitHub (repositories, issues, pull requests, releases, and search). Each
operation reports the repository it acts on, so a rule can allow
`github.com/issues.create` for one repository and no other. When an
agent needs something the package doesn't have, such as sub-issues,
issue types, or a Copilot review, GitHub's hosted MCP server has it.
Declared in a blueprint, that server becomes a package the agent imports.
The trade is in the rules, because a rule over an MCP server sees which
tool is called but not its arguments. GitHub is also the common case of
a service that won't take a login from an application it hasn't heard
of, so this is where you meet an OAuth provider.

In this tutorial we will give an agent GitHub through its hosted MCP
server, on your machine and then on a server. You need the CLI and a
GitHub account.

## Declare it

```sh
submilli blueprint init tracker
submilli blueprint add-mcp github https://api.githubcopilot.com/mcp/
```

```text
✓ created blueprint.yaml (name: tracker)
✓ added mcp server 'github' (oauth) to blueprint.yaml
  Gated by `mcp.github` (deny by default) — set its action to `allow` (optionally `filter: tool == "..."`) to use it.
  Authenticate locally: submilli mcp authenticate github --blueprint blueprint.yaml
  After applying to a server: submilli server mcp authenticate tracker github
```

```yaml title="blueprint.yaml (fragment)"
mcp:
  github:
    url: https://api.githubcopilot.com/mcp/
    auth:
      type: oauth2
```

Notice `(oauth)`. `add-mcp` asked the server how it authenticates and
wrote the answer. Until someone logs in, the blueprint is `PENDING`:

```sh
submilli mcp auth-status --blueprint blueprint.yaml
```

```text
tracker: PENDING
  github                   oauth — NOT AUTHENTICATED
```

## Register an application with GitHub

An OAuth login names the application that is asking. Linear's server
lets Submilli register one on the spot. GitHub's doesn't, so you
register a GitHub OAuth App yourself and give Submilli its id and
secret as a **provider**. It has to be an OAuth App, not a GitHub App.

On GitHub, open **Settings**, **Developer settings**, **OAuth Apps**,
and **New OAuth App**, or go straight to
[github.com/settings/applications/new](https://github.com/settings/applications/new).
To own the app as an organization, start from the organization's
settings. Fill in:

- **Application name**: what you will recognize on the approval page,
  such as `Submilli tracker`.
- **Homepage URL**: any page of yours. GitHub only displays it.
- **Authorization callback URL**: `http://127.0.0.1:8765/callback`, the
  address the login comes back to on your machine. It must match
  character for character.

Register it. GitHub shows the app's **Client ID**. Click **Generate a
new client secret** and copy the secret, which GitHub shows only once.

Put the secret in the local store, and configure the provider with a
reference to it, so the secret stays out of the provider file:

```sh
submilli secret put github_client_secret
submilli mcp provider add --match github.com --client-id Ov23li… \
  --client-secret '${secrets.github_client_secret}' --scope repo
```

```text
Value for 'github_client_secret': [hidden]
Stored secret 'github_client_secret'
✓ configured OAuth provider for github.com
```

```yaml title="~/.submilli/mcp_oauth.yaml"
providers:
- match: github.com
  client_id: Ov23li…
  client_secret: ${secrets.github_client_secret}
  scopes:
  - repo
```

The provider is matched on the host of the service's login, `github.com`,
which is not the host of the MCP server. `repo` is the scope the issue
tools need.

## Log in

```sh
submilli mcp authenticate github --blueprint blueprint.yaml
```

```text
Open this URL in your browser to authorize:

  https://github.com/login/oauth/authorize?response_type=code&client_id=Ov23li…&redirect_uri=http%3A%2F%2F127.0.0.1%3A8765%2Fcallback&state=…&code_challenge=…&code_challenge_method=S256&scope=repo

Waiting for the redirect on http://127.0.0.1:8765/callback …
✓ authenticated 'github' on blueprint 'tracker' — blueprint 'tracker' is ACTIVE
```

Open the address, approve the application, and the command finishes with
the blueprint `ACTIVE`. The login lands in the local secret store, and one
login serves each program run under the blueprint. If GitHub answers
"The redirect_uri is not associated with this application", the app's
callback URL isn't the one above. Fix it in the app's settings and open
the address again.

## Allow a tool and call it

The server's 46 tools are now a package, `@mcp/github`, and `submilli docs
@mcp/github --blueprint blueprint.yaml` lists them as declarations. One
capability, `mcp.github`, covers them all, and a rule picks tools by
name. Replace the deny rule `add-mcp` wrote with one that allows listing
issues:

```sh
submilli blueprint capability remove mcp.github
submilli blueprint capability add mcp.github --filter 'tool == "list_issues"'
```

```text
✓ removed 1 rule(s) for 'mcp.github' from caller 'main' in blueprint.yaml
✓ added allow mcp.github (filter: tool == "list_issues") to caller 'main' in blueprint.yaml
```

A program imports the server like any package:

```typescript title="issues.ts"
import github from "@mcp/github";

function main(): string {
    const page = github.list_issues({ owner: "rust-lang", repo: "rust", state: "OPEN", perPage: 5 });
    const lines: string[] = [];
    for (const issue of page.issues) {
        lines.push(`#${issue.number} ${issue.title}`);
    }
    return lines.join("\n");
}
```

```sh
submilli run --blueprint blueprint.yaml issues.ts
```

```text
warning: @mcp/github: 29 tool(s) return unknown: result schemas are unavailable or unrepresentable; consult package docs for signatures
#163678 Tracking Issue for MCP 1016: -Zasync-panic
#163677 Rust 1.99 regression: fat LTO leaves short constant string comparisons as bcmp calls
#163676 `dec2flt::decimal_seq` overflows an `i32` subtraction on a large valid decimal when overflow checks are enabled
#163675 std::io::Read incorrectly has no marked required methods
#163674 `str::parse::()` returns 1.0 for an overflowing decimal with a long fractional part
```

The warning is about types. GitHub's server publishes no result schemas,
so most tools return `unknown` and a program casts the result to a type
it declares. For 17 of them, `list_issues` among them, Submilli carries
the result type itself, so `issue.number` needs no cast. Any tool the
rule doesn't name is refused before a request leaves. [Add an
MCP server](/docs/blueprints/add-an-mcp-server) covers choosing
tools and what a rule over them can and can't see.

## The same blueprint on a server

Registered on a server, the blueprint gives each session the same
package, but the login and the provider are now the server's. The
provider goes under `mcp_oauth` in the server's config file, with the
client secret in the server's store:

```sh
submilli server secret put github_client_secret
```

```yaml title="server.yaml (fragment)"
mcp_oauth:
  providers:
  - match: github.com
    client_id: Ov23li…
    client_secret: ${secrets.github_client_secret}
    scopes:
    - repo
```

Register the blueprint and look at its logins:

```sh
submilli server blueprint apply blueprint.yaml
submilli server mcp auth-status tracker
```

```text
Added blueprint 'tracker'
tracker: PENDING
  github                   oauth — NOT AUTHENTICATED
```

The local login doesn't carry over. Until someone logs in on the server,
a program that imports the package doesn't compile, and `run-code` says
which login is missing:

```sh
submilli server run-code issues.ts --blueprint tracker
```

```text
warning: @mcp/github: server unavailable: not authenticated — run `submilli server mcp authenticate`
error: MCP server `github` is unavailable — `@mcp/github` is absent from the discovered catalog; check the blueprint's `mcp:` block and discovery warnings
```

Log in on the server. The command runs on your machine, so the browser it
needs is yours, and the login lands in the server's secret store:

```sh
submilli server mcp authenticate tracker github
```

```text
Open this URL in your browser to authorize:

  https://github.com/login/oauth/authorize?response_type=code&client_id=Ov23li…&redirect_uri=http%3A%2F%2F127.0.0.1%3A8765%2Fcallback&state=…&code_challenge=…&code_challenge_method=S256&scope=repo

Waiting for the redirect on http://127.0.0.1:8765/callback …
✓ authenticated 'github' on blueprint 'tracker' — blueprint 'tracker' is ACTIVE
```

```sh
submilli server mcp auth-status tracker
submilli server run-code issues.ts --blueprint tracker
```

```text
tracker: ACTIVE
  github                   oauth — authenticated
warning: @mcp/github: 29 tool(s) return unknown: result schemas are unavailable or unrepresentable; consult package docs for signatures
#163678 Tracking Issue for MCP 1016: -Zasync-panic
#163677 Rust 1.99 regression: fat LTO leaves short constant string comparisons as bcmp calls
#163676 `dec2flt::decimal_seq` overflows an `i32` subtraction on a large valid decimal when overflow checks are enabled
#163675 std::io::Read incorrectly has no marked required methods
#163674 `str::parse::()` returns 1.0 for an overflowing decimal with a long fractional part
```

Notice what the login is. It is one GitHub account, used by every session
of the blueprint, for every user of your application. Log in as an account
that may do what you are willing to let any user's agent do, and keep the
rule as narrow as the task needs.

## Where a per-user token belongs

One login means every user's agent acts as that account. When users must
act on GitHub as themselves, there is no login to make. Your application
holds each user's own token, and the blueprint declares it as a secret
the harness supplies when it opens the session, written straight into
the server's header:

```sh
submilli blueprint secret add GITHUB_TOKEN --harness --required
submilli blueprint add-mcp github https://api.githubcopilot.com/mcp/ \
  --header 'Authorization: Bearer ${secrets.GITHUB_TOKEN}'
```

```text
✓ declared secret 'GITHUB_TOKEN' (harness, required: true) in blueprint.yaml
✓ added mcp server 'github' (static header auth) to blueprint.yaml
  Gated by `mcp.github` (deny by default) — set its action to `allow` (optionally `filter: tool == "..."`) to use it.
```

```yaml title="blueprint.yaml (fragment)"
secrets:
  GITHUB_TOKEN:
    harness:
      required: true
mcp:
  github:
    url: https://api.githubcopilot.com/mcp/
    headers:
      Authorization: Bearer ${secrets.GITHUB_TOKEN}
```

There is no provider and no `authenticate` step. The server sends whatever
token the session was opened with, and a session opened without one is refused.
[Connect a harness](/docs/tutorials/connect-a-harness#what-every-harness-does)
shows how each harness supplies it. The rules are the same either way.
What changes is whose account the call is made as.

You have given an agent GitHub through its hosted MCP server, declared
in a blueprint and logged in once through an application you registered.
Its read tools are allowed and its write tools refused until you say
otherwise, on your machine and on a server. From here, go to
the part on [Blueprints](/docs/blueprints/start-a-blueprint) for
everything else a blueprint can grant.
