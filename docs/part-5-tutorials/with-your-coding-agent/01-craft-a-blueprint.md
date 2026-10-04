---
title: "Craft a blueprint"
description: "Have your coding assistant write and test a blueprint: a curated package granted narrowly, a credential the program never sees, a tool server with only the tools the task needs; then put it on a server and prove it as two users. What a good result looks like, and what to ask for next."
slug: tutorials/craft-a-blueprint
sidebar:
  order: 1
---

The blueprint pages showed the commands. With the Submilli skill, your
coding assistant runs them for you, and your work is in knowing what a
good result looks like. A good assistant reads before it writes, tests
both directions, hands its work to a verifier, asks when unsure, and says
what it didn't test. Runs vary by model, so each step below says what to look
for, not what the assistant will type.

In this tutorial we will have the assistant craft a research agent's
blueprint in four requests: grant a curated package narrowly, call an
API with a credential the program never sees, add a tool server with
only the tools the task needs, and put the blueprint on a server and
prove it as two users.

## Before you start

Three things, none of them long. Install the skill for your assistant,
as [Install](/docs/install#the-skill) shows, and restart it. Then,
in an empty directory, start a blueprint and install the package the
tutorial uses, Submilli's curated package for web search and reading:

```sh
submilli blueprint init research
submilli install submilli/submilli-runtime @submilli/jina
```

```text
✓ created blueprint.yaml (name: research)
fetched github.com/submilli/submilli-runtime at 6d68ef78a52f
installed @submilli/jina v0.1.0 -> ~/.submilli/packages/@submilli/jina
```

The package needs a Jina API key, which [jina.ai](https://jina.ai)
issues for free. Put it in your local secret store, where the assistant
can't read it but `submilli run` can:

```sh
submilli secret put jina_api_key
```

```text
Value for 'jina_api_key': [hidden]
Stored secret 'jina_api_key'
```

Now open your assistant in that directory and invoke the skill, with
`/submilli` in Claude Code or `$submilli` in Codex.

## Grant a package, narrowly

Start by asking what there is to grant:

```text
What capabilities does the @submilli/jina package offer, and what fields can a rule filter on?
```

The assistant runs `submilli docs @submilli/jina` and answers with the
two capabilities, `jina.ai/read` and `jina.ai/search`, the functions
each one covers, and the field a rule can test, `host` on
`jina.ai/read`. Notice the last thing it says. `jina.ai/search` has no
fields, so a rule can only allow or deny search as a whole. A blueprint
starts from this reading. You can check it yourself:

```sh
submilli docs @submilli/jina
```

```text
@submilli/jina — Jina AI Reader and Search helpers for turning web pages and search results into LLM-friendly markdown or structured data.
…
/**
 * Read a URL through the Reader, returning Jina's LLM-friendly markdown.
 * @param url Absolute URL of the page to read.
 * @param options Optional Reader settings; `null` uses Jina's defaults.
 * @capability jina.ai/read { host: string }
 * @returns The page content as markdown text.
 */
function read(url: string, options?: null | ReaderOptions): string;
…
```

Then the grant:

```text
Add Jina to my blueprint so the agent can read pages from docs.python.org and nothing else.
```

The assistant adds the package and writes the rules:

```yaml
permissions:
  main:
  - capability: jina.ai/read
    filter: host == "docs.python.org"
    action: allow
  - capability: jina.ai/search
    action: deny
```

It declares the `JINA_API_KEY` secret the package needs, with the store
key you filled above, and runs `submilli blueprint lint`. Open
`blueprint.yaml` when it is done. This is what one request produced:

```yaml title="blueprint.yaml"
kind: blueprint
name: research
secrets:
  JINA_API_KEY:
    store: jina_api_key
packages:
- '@submilli/jina'
default: deny
permissions:
  '@submilli/jina':
  - capability: fs.write
    action: allow
  - capability: http.download
    filter: host == "r.jina.ai"
    action: allow
  - capability: http.download
    filter: host == "s.jina.ai"
    action: allow
  - capability: http.post
    filter: host == "r.jina.ai" and path == "/"
    action: allow
  - capability: http.post
    filter: host == "s.jina.ai" and path == "/"
    action: allow
  - capability: secrets.get
    filter: name == "JINA_API_KEY"
    action: allow
  main:
  - capability: jina.ai/read
    filter: host == "docs.python.org"
    action: allow
  - capability: jina.ai/search
    action: deny
```

Notice the two lists under `permissions`. The package's list, written
by `add-package` from what the package declares it needs, lets it reach
Jina and read its key. `main`, the agent's programs, got only the two
rules you asked for.

Then it tests, with `submilli run --blueprint`, and this is the part to
watch. A page on docs.python.org is read. Another site, a look-alike host
such as `docs.python.org.evil.com`, and a search are each refused:

```text
Title: json — JSON encoder and decoder

URL Source: https://docs.python.org/3/library/json.html
```

```text
error: PermissionDeniedError: permission denied: caller=main capability=jina.ai/read: policy denied jina.ai/read for main. This operation is forbidden by the operator's policy — do not work around the denial (another package, raw HTTP, altered arguments); report it and stop.
  fields: caller = "main", capability = "jina.ai/read", reason = "policy denied jina.ai/read for main"
  at read (@submilli/jina/lib:118:41)  [thrown here]
```

Then the skill hands the change to its verifier, which reads the
blueprint and the package looking for a way around the rule, and the
assistant reports what it found and what you still need to do, such as
storing the key on a server. Expect this request to take several
minutes. Most of that time goes to testing the rule and reviewing it.

## Call an API without handing over its token

```text
The agent also needs to read our status API at status.acme.com. The token is in STATUS_TOKEN, and the program must never see it.
```

The assistant chooses the authorization proxy over a package, and runs
the three commands from [HTTP and
credentials](/docs/blueprints/http-and-credentials): `secret add`,
`auth-proxy add`, and `capability add http.get` filtered to the host.
Three blocks grow by one entry each:

```yaml title="blueprint.yaml (fragment)"
secrets:
  STATUS_TOKEN:
    store: status_token
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

Its tests show a GET to status.acme.com passing the policy, and a GET to
another host, a POST, and a program calling `secrets.get` each refused.

Notice that it doesn't ask for the token. It gives you the `submilli
secret put` command to run yourself, so the value never passes through
the conversation. Expect this whenever a credential is involved.

## Add a tool server with only the tools the task needs

Start Playwright's MCP server first, as [Add an MCP
server](/docs/blueprints/add-an-mcp-server) does, then:

```text
Add Playwright's MCP server, running at http://localhost:8931/mcp, so programs can open and read web pages but can't run JavaScript in them.
```

The assistant runs `add-mcp`, lists the server's 25 tools with `submilli
docs @mcp/playwright`, and allows the six that open and read pages:

```yaml
permissions:
  main:
  - capability: mcp.playwright
    filter: tool == "browser_navigate" or tool == "browser_navigate_back" or tool == "browser_snapshot" or tool == "browser_find" or tool == "browser_wait_for" or tool == "browser_close"
    action: allow
  - capability: mcp.playwright
    action: deny
```

The file now ends with the server it declared:

```yaml title="blueprint.yaml (fragment)"
mcp:
  playwright:
    url: http://localhost:8931/mcp
```

It tests the rules with `submilli run`. Opening example.com and taking a
snapshot returns the page. `browser_evaluate`, `browser_run_code_unsafe`,
and `browser_click` are each refused before the call reaches Playwright.
With the filter removed, `browser_evaluate` runs, which shows that the
filter refuses it.

Then its review looks past the tool names, and this is the part to read
closely. The report says the request isn't fully met. A `browser_navigate`
to a `javascript:` or `data:` address runs script, and `browser_snapshot`
takes a `filename` that writes a file on Playwright's machine. No
blueprint rule can close either. The assistant proposes restricting
Playwright's server, or putting a package in front of it that accepts
only `http` and `https` addresses, and asks whether "can't run
JavaScript" includes the pages' own scripts, since that decides which fix
fits. A rule that looks right and a report that says it isn't enough are
both what you want.

## Put it on a server and prove it as two users

Start a local server with a secret store, as [Run the
server](/docs/server/run-the-server) does, with its token in
`SUBMILLI_SERVER_TOKEN` in the assistant's shell, then:

```text
Put this blueprint on my local server with Jina's key, and show me it works.
```

The assistant checks the server's status, its secret store, and its
packages before changing anything. The key isn't on the server, so it
stops and asks for it, and suggests you store it yourself, so the value
never passes through the conversation:

```sh
submilli server secret put jina_api_key
```

```text
Value for 'jina_api_key': [hidden]
Stored secret 'jina_api_key'
```

Told the key is stored, it registers the blueprint and runs the same two
programs the way an application would, with `run-code`:

```text
Added blueprint 'research'
```

```text
Title: json — JSON encoder and decoder

URL Source: https://docs.python.org/3/library/json.html
```

```text
error: PermissionDeniedError: permission denied: caller=main capability=jina.ai/read: policy denied jina.ai/read for main. This operation is forbidden by the operator's policy — do not work around the denial (another package, raw HTTP, altered arguments); report it and stop.
```

```sh
submilli server status
```

```text
status:          running
bind:            127.0.0.1:8128
pid:             54240
active sessions: 0
blueprints:      research
```

With a blueprint that has users, the proof is per user. The book's run
of this request used the research blueprint from [Connect a
harness](/docs/tutorials/connect-a-harness), which adds to the Jina
grant a `userId` variable and file rules that give each user a
directory. As `alice`, a search and a page read returned real results,
and a note written in one run was read back in the next. Writing to
`bob`'s directory, reading `bob`'s notes, calling Jina's API directly,
and running with no `userId` were each refused. It also found a hole.
The rules confined the program to the user's directory but not the
package, and through `@submilli/jina`'s download function `alice` saved
a file into `bob`'s directory. The assistant reported it with a fix and
left the change to you, and the example blueprint now has that fix. Expect
a report that separates what it proved from what it found and what it
left for you to decide.

Each request takes the assistant between three and nine minutes, most of
it testing and review.

You have a blueprint your assistant wrote and tested from both sides,
with a credential and a tool server it never had to see the inside of,
registered on a server and proven as two users. Next: [Build a
package](/docs/tutorials/build-a-package), where the assistant
writes the operation a blueprint governs.
