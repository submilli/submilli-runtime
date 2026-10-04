---
title: "Register a blueprint"
description: "How to put a blueprint on a server: install its packages and store its secrets first, register it, prove it with a program, and update or remove it later, knowing what registration checks and what remove costs."
slug: server/register-a-blueprint
sidebar:
  order: 3
---

A blueprint reaches a server as a file you register. The server keeps a
copy and never reads the file again. Registration catches mistakes, so a
blueprint whose secret isn't in the store yet fails here and not on the
first program. An edit takes effect when you register it, and removing a
blueprint ends the sessions bound to it.

This guide shows you how to register a blueprint on a server. The example
is the support blueprint from [Start a
blueprint](/docs/blueprints/start-a-blueprint). Substitute yours.

## Put its dependencies in place

The server has a separate package store, filled from GitHub:

```sh
submilli server packages install acme/billing-package @acme/billing
```

```text
installed @acme/billing @ 3f9c2a1b7e40
```

The server fetches the repository, builds the package, and pins it to the
commit it resolved. A server on the same machine as your CLI also reads
the CLI's store, so a package you published locally is already there.
For a private repository, refer to [Install private
packages](/docs/server/install-private-packages). The [CLI
reference](/docs/reference/cli) covers pinning and upgrading.

The server prints the build's warnings and installs the package anyway.
For a package you didn't write, a warning such as a `check` that
disagrees with its `@capability` tag is a gap in what your blueprint can
enforce. Refuse such a package:

```sh
submilli server packages install --deny-warnings acme/billing-package @acme/billing
```

With a warning, the command fails and the server installs nothing. To
apply this rule to all installs on the server, whatever the caller asks,
start the server with `SUBMILLI_DENY_WARNINGS=1`.

Each `store:` secret the blueprint declares must be in the server's
store before registration. The blueprint names the secret as the package
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

Registration checks the blueprint, so a mistake fails here and not on
the first program. The YAML and its filters must parse. Each `store:`
secret must exist in the server's store. Each package in `packages:`,
and each package those depend on, must be installed. Each package's list
must hold the rules the package requires, as `lint` checks. Each named
volume the blueprint mounts must be one the server declares, with no more
access than the server allows. Registered before the secret was put, the
same file is refused:

```text
error: secret check failed: missing secret 'BILLING_API_KEY'
```

Before the package was installed:

```text
error: package check failed: package `@acme/billing` is not installed; install it with `submilli server packages install <org/repo> @acme/billing`
```

And with the package's `secrets.get` rule deleted from its list:

```text
error: package check failed: package `@acme/billing` requires `secrets.get` with filter `name == "BILLING_API_KEY"`, but `permissions.@acme/billing` has no matching rule
```

The checks run when the blueprint is registered. If a package is
uninstalled afterwards, the first program that imports it fails.

A blueprint that declares MCP servers registers the same way, but the
server has to reach them itself and log in to any that use OAuth. Until
it does, the blueprint is `PENDING` and runs programs without those
servers. [Add an MCP server](/docs/blueprints/add-an-mcp-server#register-it-on-a-server)
covers the server side.

## Prove it

Run a program the way an application would, naming the blueprint and
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

`remove <name>` unregisters a blueprint and ends the open sessions bound
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
a blueprint under open sessions, `apply` the new version.
