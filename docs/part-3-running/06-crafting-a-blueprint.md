---
title: "Crafting a blueprint"
description: "What a blueprint is, and how to build one with the CLI, block by block: packages, rules, variables, secrets, files, and models for one agent."
slug: blueprints
sidebar:
  order: 6
---

A blueprint is one YAML file that describes everything one agent's programs
may do. The server keeps it under a name; your application or harness names it
when it opens a session, and from then on every gated call the program makes is
answered from that file. What the file doesn't grant, the program can't do.
That is the whole idea: the environment an agent's code runs in is written
down, once, by you, outside the model, and the runtime reads it on every call.

The quickstart wrote a blueprint with one rule. This chapter builds a complete
one for the support agent from [how Submilli works](/docs/how-submilli-works):
it may credit the customer it is serving, read a status page with a
credential, keep notes between programs, and hand part of its work to a model.
Each step is one CLI command, what it wrote, and what that changes. The
commands edit `blueprint.yaml` in the current directory and rewrite it each
time, so comments you add by hand don't survive them.

## Start from nothing allowed

```sh
submilli blueprint init support
```

The file it writes, minus its comments, permits nothing:

```yaml title="blueprint.yaml"
kind: blueprint
name: support
default: deny
permissions:
  main: []
```

`name` is what the application and the server call it. `permissions` is a map
from *caller* to a list of rules; `main` is the generated program. `default` is
the answer when no rule matches. Leaving `default` out means `deny` as well, so
a blueprint containing only a name denies everything.

Under this file, a program that computes and returns a value runs fine.
Anything that reaches outside the instance, a file, a request, a package
operation, fails with a permission error. Every step from here on widens that.

## Add a package

The agent credits customers through `@acme/billing`, the package from [how
Submilli works](/docs/how-submilli-works), built and installed locally the way
the quickstart's package was. A program can import a package only if the
blueprint lists it, so the first step is to list this one:

```sh
submilli blueprint add-package @acme/billing --no-capabilities
```

```text
warning: blueprint.yaml: package `@acme/billing` requires secret `BILLING_API_KEY`, but `secrets:` does not declare it
✓ added @acme/billing to blueprint.yaml
  2 provided capabilities not selected — denied by `default: deny`
  added 2 rules to caller `@acme/billing` (default allow):
    allow http.get (filter: host == "billing.internal.example.com" and method == "GET")
    allow secrets.get (filter: name == "BILLING_API_KEY")
```

```yaml title="blueprint.yaml (fragment)"
packages:
- '@acme/billing'
permissions:
  '@acme/billing':
  - capability: http.get
    filter: host == "billing.internal.example.com" and method == "GET"
    action: allow
  - capability: secrets.get
    filter: name == "BILLING_API_KEY"
    action: allow
  main: []
```

Two things happened. The package is listed, so the import resolves. And the
package got a caller list of its own: a package is code too, and this one's
invoice operation fetches from the billing service with an API key, so the
package's declarations say it requires `http.get` to that host and
`secrets.get` for that key, and the command wrote exactly those rules. Its
warning is about the key itself, declared two steps from now.

What the command did not do is grant the program anything; `--no-capabilities`
left `main` empty. Listing a package never does that by itself. Its operations
are capabilities, and each one the program may call needs a rule under `main`.

## Grant the operation, tied to the session

A rule has three fields: `capability`, the name the package or the standard
library gave the operation; an optional `filter` over the fields the operation
reports; and `action`, `allow` or `deny`. The rule we want lets the program
credit the customer this session is about, and only a premium one. Which
customer that is has to come from outside the program, so first declare a
**variable** for it:

```sh
submilli blueprint variable add customerId --required
```

```yaml title="blueprint.yaml (fragment)"
variables:
  customerId:
    required: true
```

The rule needs the operation's capability name and the fields its filter can
test. Both come from the package's declarations, and `capability list` prints
them for one package:

```sh
submilli blueprint capability list @acme/billing
```

```text
@acme/billing
  acme.com/credits.apply — Add a goodwill credit to a customer's account.
      fields: amount: number, customerClass: string, customerId: string
  acme.com/invoices.latest — Fetch the size of a customer's latest invoice from the billing service.
      fields: customerId: string
```

`customerId` and `customerClass` are the fields to test, so the rule is:

```sh
submilli blueprint capability add acme.com/credits.apply \
  --filter 'customerId == ${vars.customerId} and customerClass == "premium"'
```

```text
✓ added allow acme.com/credits.apply (filter: customerId == ${vars.customerId} and customerClass == "premium") to caller 'main' in blueprint.yaml
```

```yaml title="blueprint.yaml (fragment)"
permissions:
  main:
  - capability: acme.com/credits.apply
    filter: customerId == ${vars.customerId} and customerClass == "premium"
    action: allow
```

The application binds `customerId` when it opens a session, and the program
can neither read nor change it ([how Submilli works](/docs/how-submilli-works)
explains why). One blueprint therefore serves every customer; the binding
changes, the file doesn't. A variable is a string, and it is either
`--required` or has a `--default`. A session that omits a required variable is
refused before any program runs:

```text
invalid variables: required variable 'customerId' was not supplied
```

When a program calls a gated operation, the runtime finds the caller's list,
walks it top to bottom, and takes the first rule whose capability matches by
name and whose filter is absent or true. If none matches, `default` decides.
Names match exactly: there is no `fs.*`, so allowing `fs.write` doesn't allow
`fs.mkdir`, and the program learns that on its first call:

```text
error: PermissionDeniedError: permission denied: caller=main capability=fs.mkdir: policy denied fs.mkdir for main.
```

Filters compare a field with `==`, `!=`, `<`, `<=`, `>`, `>=`, test a pattern
with `glob` or `matches`, and combine with `and`, `or`, `not`. A field the
operation didn't report never matches. `capability list` with no argument
prints the standard library's capabilities and their fields as well; the full
grammar is in [security and permissions](/docs/security).

## Declare the secret

```sh
submilli blueprint secret add BILLING_API_KEY --store billing_api_key
```

```yaml title="blueprint.yaml (fragment)"
secrets:
  BILLING_API_KEY:
    store: billing_api_key
```

A secret entry is a name and where the runtime finds the value: `--store` for
the secret store, `--harness` for a value the application supplies per
session, `--env` for an environment variable, `--file` for a file. The
blueprint holds the name only. The value goes into the store:

```sh
submilli secret put billing_api_key
```

```text
Value for 'billing_api_key': [hidden]
Stored secret 'billing_api_key'
```

The command prompts for the value and doesn't echo it, so it never appears in
a command line or the terminal's scrollback. In a script, pipe the value in
instead: `submilli secret put billing_api_key < key.txt`.

A blueprint can only name a secret it declares, and a declared secret whose
value can't be found is refused when the blueprint is loaded, not when a
program first needs it.

`secrets.get` is the one capability no rule can grant to `main`. The value goes
to the package, and the program gets what the package returns.

## Call an endpoint with a credential

Sometimes there is no package, only an HTTP API. The program may call it under
an `http.get` rule, and the **auth proxy** adds the credential on the way out:

```sh
submilli blueprint secret add STATUS_TOKEN --store status_token
submilli secret put status_token
submilli blueprint auth-proxy add --host status.acme.com --bearer STATUS_TOKEN
submilli blueprint capability add http.get --filter 'host == "status.acme.com"'
```

```yaml title="blueprint.yaml (fragment)"
auth_proxy:
- host: status.acme.com
  auth:
    bearer: STATUS_TOKEN
permissions:
  main:
  - capability: http.get
    filter: host == "status.acme.com"
    action: allow
```

The permission check runs first; then, for a request whose host matches an
`auth_proxy` entry exactly, the runtime adds the header. The program sends a
plain `http.get("https://status.acme.com/…")` and receives the response; the
token never enters the instance. `--bearer` and `--basic-username` with
`--basic-password` cover the common cases; `--header` and `--query` with
`${secrets.NAME}` values cover the rest.

## Let programs remember

Each program starts with fresh memory. The blueprint decides what else it
finds: files, and the key-value store of `submilli:session`. The rules come
from the CLI; the `vfs` and `idle_timeout` lines have no command, so add them
by hand:

```sh
submilli blueprint capability add fs.read
submilli blueprint capability add fs.stat
submilli blueprint capability add fs.mkdir --filter 'path == "/notes"'
submilli blueprint capability add fs.write --filter 'path glob "/notes/*"'
submilli blueprint capability add session.read
submilli blueprint capability add session.write
```

```yaml title="blueprint.yaml (fragment)"
vfs: per_session
idle_timeout: 1h
```

`vfs` is the program's filesystem, and it has four modes:

| Mode | The program's `/` is |
| --- | --- |
| `none` | Nothing; every `submilli:fs` call fails |
| `ephemeral` (the default) | A scratch directory created for the run and deleted after it |
| `per_session` | A directory that lasts as long as the session |
| `persistent` | A volume the server operator declared, kept across sessions and restarts |

A **session** is what the application or harness opens for one conversation
with the agent, and then runs each program inside. Everything under
`per_session`, files and session state alike, lives exactly that long. A
program run outside a session gets a session of its own that closes when it
returns, so nothing survives it. `idle_timeout` closes a session nobody has
used for that long; the default is 24 hours. [Connecting to your
harness](/docs/harness) shows how a session is opened.

With this blueprint, a program that writes `/notes/northwind.md` and sets a
session key is followed by one that reads both back, and a program in a new
session finds neither.

## Let the program commit

`submilli:git` works on repositories inside the program's filesystem. Every
commit it makes is authored by the blueprint, not the program, so the module
stays disabled until the blueprint gives it an identity:

```sh
submilli blueprint git set --name "Support Agent" --email agent@acme.example
```

```text
Configured Git in blueprint.yaml. Configure grants with `submilli blueprint capability add`.
```

```yaml title="blueprint.yaml (fragment)"
git:
  identity:
    name: Support Agent
    email: agent@acme.example
```

A value may include `${vars.NAME}`, so `--name 'Support Agent (${vars.customerId})'`
records the session in the author. The operations are capabilities like any
other, each reporting the repository `path`; this agent keeps its notes under
version control and nothing else:

```sh
submilli blueprint capability add git.init --filter 'path == "/notes"'
submilli blueprint capability add git.stage --filter 'path == "/notes"'
submilli blueprint capability add git.commit --filter 'path == "/notes"'
submilli blueprint capability add git.read --filter 'path == "/notes"'
```

Remotes are HTTPS only, under `git.remote`, `git.fetch`, and `git.checkout`,
whose `remote` field lets a rule pin the hosts a program may clone from or
fetch. A private repository needs two more things: `--username` on `git set`,
and a secret named `GIT_TOKEN`. The token is sent only under
`git.authenticate`, and the program never sees it.

## Let the program call a model

`submilli:llm` ([the standard library](/docs/standard-library) shows it in use)
sends prompts to models the blueprint declares. The `llm` block is the catalog:
a model it doesn't name can't be called. Declare the key with the CLI, write
the block by hand, and grant the capability:

```sh
submilli blueprint secret add ANTHROPIC_API_KEY --store anthropic_api_key
submilli secret put anthropic_api_key
```

```yaml title="blueprint.yaml (fragment)"
llm:
  providers:
    anthropic:
      type: anthropic
      api_key: ${secrets.ANTHROPIC_API_KEY}
  models:
    claude-haiku-4-5:
      provider: anthropic
      description: "Cheap and fast; use for bulk per-item classification."
    claude-sonnet-5:
      provider: anthropic
```

```sh
submilli blueprint capability add llm.call --filter 'model glob "claude-*"'
```

One capability, `llm.call`, covers `call`, `batch`, and `models()`, and the
`model` field is how a rule tells them apart. The descriptions reach the model
writing the program, so they are where you say which model is for what.

## Check it, and see what the agent sees

```sh
submilli blueprint lint blueprint.yaml
```

```text
warning: blueprint.yaml: package `@acme/billing` provides `acme.com/invoices.latest`, but `permissions.main` has no matching rule
✓ blueprint.yaml is valid
```

`lint` parses the file and checks it against the packages installed locally: a
package that needs an operation its own list doesn't allow, a secret a package
reads that `secrets` doesn't declare, an operation a package provides that
`main` has no rule for. The warning here is deliberate; this agent doesn't
fetch invoices. `lint --fix` adds the missing package rules. A misspelled
capability name is a rule that never matches, and `lint` doesn't catch it;
`capability add` refuses a name it doesn't know.

The blueprint also shapes what the agent is told. The description of the tool
that runs code is assembled from it: which hosts the program may reach, what
its filesystem is, which models it may call.

```sh
submilli blueprint prompt
```

```text
Sandbox: File system per_session — a sandbox that persists across calls in this session.
Network (`submilli:http`): GET → status.acme.com.
```

That is the quickest way to read your policy as the model will.

## The whole file

As the CLI leaves it. It orders the blocks itself and writes `1h` back as
`'3600s'`:

```yaml title="blueprint.yaml"
kind: blueprint
name: support
idle_timeout: '3600s'
vfs:
  mode: per_session
secrets:
  ANTHROPIC_API_KEY:
    store: anthropic_api_key
  BILLING_API_KEY:
    store: billing_api_key
  STATUS_TOKEN:
    store: status_token
variables:
  customerId:
    required: true
packages:
- '@acme/billing'
auth_proxy:
- host: status.acme.com
  auth:
    bearer: STATUS_TOKEN
git:
  identity:
    name: Support Agent
    email: agent@acme.example
default: deny
permissions:
  '@acme/billing':
  - capability: http.get
    filter: host == "billing.internal.example.com" and method == "GET"
    action: allow
  - capability: secrets.get
    filter: name == "BILLING_API_KEY"
    action: allow
  main:
  - capability: acme.com/credits.apply
    filter: customerId == ${vars.customerId} and customerClass == "premium"
    action: allow
  - capability: http.get
    filter: host == "status.acme.com"
    action: allow
  - capability: fs.read
    action: allow
  - capability: fs.stat
    action: allow
  - capability: fs.mkdir
    filter: path == "/notes"
    action: allow
  - capability: fs.write
    filter: path glob "/notes/*"
    action: allow
  - capability: session.read
    action: allow
  - capability: session.write
    action: allow
  - capability: git.init
    filter: path == "/notes"
    action: allow
  - capability: git.stage
    filter: path == "/notes"
    action: allow
  - capability: git.commit
    filter: path == "/notes"
    action: allow
  - capability: git.read
    filter: path == "/notes"
    action: allow
  - capability: llm.call
    filter: model glob "claude-*"
    action: allow
llm:
  providers:
    anthropic:
      type: anthropic
      api_key: ${secrets.ANTHROPIC_API_KEY}
  models:
    claude-haiku-4-5:
      provider: anthropic
      description: Cheap and fast; use for bulk per-item classification.
    claude-sonnet-5:
      provider: anthropic
```

Thirteen blocks exist in all; the one this chapter skipped is `mcp`, which
`add-mcp` writes to make an MCP server you already run importable as a
package, covered in [connecting to your harness](/docs/harness). Anything else
in the file is a parse error, so a typo can't silently grant or withhold
anything.

The file is finished. Registering it with a running server, so applications
can name it, is covered in [Submilli server](/docs/server).

Next: [using the CLI](/docs/cli).
