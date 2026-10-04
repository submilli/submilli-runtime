# Harness examples

The same agent, written once per harness: a research assistant that searches
the web, reads pages, and keeps a notebook for each user. The model writes
programs and `submilli-server` runs them. The examples accompany the book
chapter "Connecting to your harness".

| Directory | Harness | Reaches the server over |
|:----------|:--------|:------------------------|
| `mastra/` | Mastra (TypeScript) | MCP |
| `deepagents/` | LangChain deepagents (Python) | MCP |
| `openai-agents/` | OpenAI Agents SDK (Python) | MCP |
| `claude-agent-sdk/` | Claude Agent SDK (TypeScript) | MCP |
| `vercel-ai-sdk-http/` | Vercel AI SDK (TypeScript) | The HTTP API; `submilli.ts` builds the tools |

`blueprint.yaml` is the policy they all run under: the `@submilli/jina`
package for search and reading, and a named `notes` volume of which each
session sees only the directory of the user its `userId` variable names,
at `/notes`. `prompt.txt` is the agents' brief; `note.ts` is
a small program the checks run in a model's place.

## Before you run one

The server needs the volume, a secret store for the Jina and model
provider keys, and the package. From this directory:

```sh
mkdir -p "$HOME/submilli-notes"
printf 'volumes:\n  notes:\n    kind: local-path\n    path: %s\n    size_limit: unlimited\n' "$HOME/submilli-notes" > server.yaml
head -c 32 /dev/urandom | base64 > store.key
export SUBMILLI_SERVER_TOKEN=$(openssl rand -hex 32)

submilli-server --config server.yaml --secret-store-key-file store.key &

submilli server packages install submilli/submilli-runtime @submilli/jina
submilli server secret put jina_api_key
submilli server secret put anthropic_api_key
submilli server blueprint apply blueprint.yaml
```

From a checkout of this repository, `submilli build publish-local -p
@submilli/jina`, run at its root before the server starts, installs the
package from source instead. `secret put` prompts for each key; Jina issues one at <https://jina.ai>.
The checks make no request to Jina or the model provider, so any value
will do for them.

A server somewhere else is named with `SUBMILLI_SERVER=http://host:port`.

## Run an agent

Each agent needs the API key of its model provider.

```sh
cd mastra && npm install
ANTHROPIC_API_KEY=... npm run agent
```

```sh
cd deepagents && pip install -r requirements.txt
ANTHROPIC_API_KEY=... python agent.py
```

```sh
cd openai-agents && pip install -r requirements.txt
OPENAI_API_KEY=... python agent.py
```

```sh
cd claude-agent-sdk && npm install
ANTHROPIC_API_KEY=... npm run agent
```

```sh
cd vercel-ai-sdk-http && npm install
ANTHROPIC_API_KEY=... npm run agent
```

## Check one without a model

Every directory has a check that needs no model API key. It runs the agent
against the local server with a scripted model that calls the execute tool
with `note.ts`, and asserts three things: two users who write the same note
each see only their own, a connection that names no user is refused, and so
is one whose token the server does not know. The deepagents check also lists
the user's notes with `submilli__files__list`.

```sh
npm run check       # the TypeScript examples
python check.py     # the Python examples
```

The Claude Agent SDK has no scripted model, so its check stops short of the
model: it asserts that the agent connects and is offered the execute tool, and
that a connection without a user, or with an unknown token, fails.
