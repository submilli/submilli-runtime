# Quickstart example

Every artifact the separately maintained quickstart chapter
has the reader author, in runnable form. `submilli.toml` and
`package/capabilities.yaml` are not in the table because you never write them —
`submilli build` produces them.

| File | What it is |
|:-----|:-----------|
| `package/` | `@acme/billing` — one operation, `listCharges(customerId)`, over a fixture. |
| `blueprint.yaml` | The policy: `customerId` as a required session variable, and the operation granted only for it. |
| `server.yaml` | The server's config: an `admin` token for you and a `user` token for the application, each read from an environment variable. |
| `total.ts` | The program the agent writes for the job it was asked to do. |
| `total-injected.ts` | The program it writes after reading the injected support ticket. |
| `app.mjs` | The application: sends the `user` token, binds `customerId` per request, and posts to `/v1/execute`. |
| `agent.py` | Optional: the same ticket handed to a real deepagents agent over MCP. Needs `requirements.txt` and a model API key; not exercised by `verify.sh`. |

## Running it

```
./verify.sh
```

Walks the chapter's journey end to end and asserts both outcomes — the total and
the denial — plus controls proving the denial comes from the filter rather than
from something else. It runs against a throwaway `SUBMILLI_HOME`, so it never
touches your real package store. It generates the two tokens itself, starts the
server on `127.0.0.1:8128` with `server.yaml`, and also checks that a missing or
unknown token is refused and that the `user` token cannot apply a blueprint. It
needs `node`, `curl`, `openssl`, and `python3`.

By default the binaries come from `cargo run` against this checkout. Point it at
installed ones with:

```
SUBMILLI=submilli SUBMILLI_SERVER=submilli-server ./verify.sh
```

## The optional agent

`agent.py` replaces the hand-pasted `total-injected.ts` with a real model, over
MCP, against the same blueprint. It sends two headers: `Authorization`, with
the `user` token from `SUBMILLI_USER_TOKEN`, and `submilli-variables`, which
binds `customerId` — the channel for MCP clients that cannot set `initialize`
`_meta`, which includes langchain. It needs a server running as the chapter
starts it; never give it the `admin` token.

```
pip install -r requirements.txt
export GOOGLE_API_KEY=...
export SUBMILLI_USER_TOKEN=...   # the value the server was started with
python agent.py
```

`verify.sh` leaves this alone: it needs a key, costs money, and a model only
takes the injection bait some of the time. The snippet the chapter prints is
drift-checked when SUBMILLI_QUICKSTART_CHAPTER points to the chapter.

## Why this needs the server

`submilli run` accepts a blueprint but binds no session variables, and an
unresolved `${vars.NAME}` is a silent non-match rather than an error — so under
`submilli run` this blueprint denies the legitimate call too. Binding a variable
per request is what `submilli-server` adds, and it is the whole point of the
chapter.

## Checking the book

The book is maintained separately. To compare its code blocks with these files:

```sh
SUBMILLI_QUICKSTART_CHAPTER=/path/to/02-quickstart.md ./verify.sh --blocks-only
```
