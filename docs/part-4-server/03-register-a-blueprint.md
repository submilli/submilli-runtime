---
title: "Register a Blueprint"
description: "How to put a Blueprint on a server: install its Packages and store its secrets first, register it, prove it with a program, and update or remove it later, knowing what registration checks and what remove costs."
slug: server/register-a-blueprint
sidebar:
  order: 3
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "65cb04bda9090f2ef0513284b46baf5f7415c4e37a7962a8231b6beaf96cba9d"
  confirmedAt: "2026-10-05T10:59:51.485Z"
---

A Blueprint reaches a server as a file you register. The server keeps a
copy and never reads the file again. Registration catches mistakes, so a
Blueprint whose secret isn't in the store yet fails here and not on the
first program. An edit takes effect when you register it, and removing a
Blueprint ends the sessions bound to it.

This guide shows you how to register a Blueprint on a server. The example
is the support Blueprint from [Start a
Blueprint](/docs/blueprints/start-a-blueprint). Substitute yours.

## Put its dependencies in place

The server has a separate Package store, filled from GitHub:

```sh
submilli server packages install acme/billing-package @acme/billing
```

```text
installed @acme/billing @ 3f9c2a1b7e40
```

The server fetches the repository, builds the Package, and pins it to the
commit it resolved. A server on the same machine as your CLI also reads
the CLI's store, so a Package you published locally is already there.
For a private repository, refer to [Install private
Packages](/docs/server/install-private-packages). The [CLI
reference](/docs/reference/cli) covers pinning and upgrading.

The server prints the build's warnings and installs the Package anyway.
For a Package you didn't write, a warning such as a `check` that
disagrees with its `@capability` tag is a gap in what your Blueprint can
enforce. Refuse such a Package:

```sh
submilli server packages install --deny-warnings acme/billing-package @acme/billing
```

With a warning, the command fails and the server installs nothing. To
apply this rule to all installs on the server, whatever the caller asks,
start the server with `SUBMILLI_DENY_WARNINGS=1`.

Each `store:` secret the Blueprint declares must be in the server's
store before registration. The Blueprint names the secret as the Package
reads it, and the store key it comes from:

```yaml title="blueprint.yaml (fragment)"
secrets:
  BILLING_API_KEY:
    store: billing_api_key
```

```sh
submilli server secret put billing_api_key
```

```text
Value for 'billing_api_key': [hidden]
Stored secret 'billing_api_key'
```

## Register it

```sh
submilli server blueprint apply blueprint.yaml
```

```text
Added blueprint 'support'
```

Registration checks the Blueprint, so a mistake fails here and not on
the first program. The YAML and its filters must parse. Each `store:`
secret must exist in the server's store. Each Package in `packages:`,
and each Package those depend on, must be installed. Each Package's list
must hold the rules the Package requires, as `lint` checks. Each named
volume the Blueprint mounts must be one the server declares, with no more
access than the server allows. Registered before the secret was put, the
same file is refused:

```text
error: secret check failed: missing secret 'BILLING_API_KEY'
```

Before the Package was installed:

```text
error: package check failed: package `@acme/billing` is not installed; install it with `submilli server packages install <org/repo> @acme/billing`
```

And with the Package's `secrets.get` rule deleted from its list:

```text
error: package check failed: package `@acme/billing` requires `secrets.get` with filter `name == "BILLING_API_KEY"`, but `permissions.@acme/billing` has no matching rule
```

The checks run when the Blueprint is registered. If a Package is
uninstalled afterwards, the first program that imports it fails.

A Blueprint that declares MCP servers registers the same way, but the
server has to reach them itself and log in to any that use OAuth. Until
it does, the Blueprint is `PENDING` and runs programs without those
servers. [Add an MCP server](/docs/blueprints/add-an-mcp-server#register-it-on-a-server)
covers the server side.

## Prove it

Run a program the way an application would, naming the Blueprint and
binding its variable:

```sh
submilli server run-code credit.ts --blueprint support --var customerId=cus_VMQR3azuTWVAWs
```

```text
credited 1500 cents, balance -9400
```

## Update or remove it

`apply` registers or replaces. Run it again after an edit:

```text
Updated blueprint 'support'
```

`add` is `apply` for a name that must be new, and refuses one already
taken. `list` prints the registered names and `show <name>` prints the
YAML the server holds.

`remove <name>` unregisters a Blueprint and ends the open sessions bound
to it. On a live server that cuts off the people using it:

```sh
submilli server status
submilli server blueprint remove support
submilli server status
```

```text
status:          running
bind:            127.0.0.1:8128
pid:             18999
active sessions: 1
blueprints:      support
Removed blueprint 'support'
status:          running
bind:            127.0.0.1:8128
pid:             18999
active sessions: 0
blueprints:      (none)
```

A client that comes back to its session gets `unknown session`. To change
a Blueprint under open sessions, `apply` the new version.
