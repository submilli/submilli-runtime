---
title: "Register a blueprint"
description: "How to put a blueprint on a server: install its packages and store its secrets first, register it, prove it with a program, and update or remove it later, knowing what registration checks and what remove costs."
slug: next/server/register-a-blueprint
pagefind: false
sidebar:
  order: 3
  hidden: true
---

A blueprint reaches a server as a file you register; the server keeps its
own copy and never reads the file again. Registration is where a mistake
is caught, so a blueprint whose secret isn't in the store yet fails here
rather than on the first program, and it is where an edit takes effect
and where removing one ends the sessions bound to it.

This guide shows you how to register a blueprint on a server: put the
packages and secrets it depends on in place, register it, prove it with a
program, and update or remove it later. The example is the support
blueprint from [Start a
blueprint](/docs/next/blueprints/start-a-blueprint); substitute yours.

## Put its dependencies in place

The server has a package store of its own, filled from GitHub:

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
packages](/docs/next/server/install-private-packages); refer to the [CLI
reference](/docs/next/reference/cli) for pinning and upgrading.

Every `store:` secret the blueprint declares must be in the server's
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

Registration checks the blueprint, so a mistake fails here rather than on
the first program: the YAML and every filter must parse, every `store:`
secret must exist in the server's store, every package in `packages:`
and every package those depend on must be installed, each package's own
list must hold the rules it requires, as `lint` checks, and every named
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

The checks run when the blueprint is registered; a package uninstalled
afterwards still fails the first program that imports it.

A blueprint that declares MCP servers registers the same way, but the
server has to reach them itself, and log in to any that use OAuth: until
it does, the blueprint is `PENDING` and runs programs without those
servers. [Add an MCP server](/docs/next/blueprints/add-an-mcp-server#register-it-on-a-server)
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

`apply` registers or replaces; run it again after an edit:

```text
Updated blueprint 'support'
```

`add` is `apply` for a name that must be new, and refuses one already
taken. `list` prints the registered names and `show <name>` prints the
YAML the server holds.

`remove <name>` unregisters a blueprint and ends every open session bound
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
a blueprint under open sessions, `apply` the new version instead.
