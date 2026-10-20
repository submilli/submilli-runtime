# @submilli/linear-agent

A small outbound API for publishing public progress, actions, questions,
completed answers, and errors in existing Linear agent sessions. Includes
labeled choices, account-linking requests, paginated history, and external links.
See [the API guide](docs/readme.md) for examples and execution semantics.

## Blueprint setup

Build/install the package, then run:

```sh
submilli blueprint add-package @submilli/linear-agent --all-capabilities
```

This discovers its `@submilli/linear` dependency and scaffolds all three layers
of permissions. Review the generated rules: grant main only the intent helpers
it needs and restrict the wrapper's Linear operation grants to permitted teams.
Declare `LINEAR_API_KEY` as a required harness secret. For example, a blueprint
that permits progress and completion for one team:

```yaml
kind: blueprint
name: linear-agent-example
packages:
  - '@submilli/linear-agent'
secrets:
  LINEAR_API_KEY:
    harness:
      required: true
default: deny
permissions:
  main:
    - capability: linear-agent.thought
      action: allow
    - capability: linear-agent.respond
      action: allow
  '@submilli/linear-agent':
    - capability: linear.app/createAgentActivity
      action: allow
      filter: teamId == "YOUR_TEAM_UUID"
  '@submilli/linear':
    - capability: secrets.get
      action: allow
      filter: name == "LINEAR_API_KEY"
    - capability: http.post
      action: allow
      filter: host == "api.linear.app" and path == "/graphql"
```

Reads require `linear.app/getAgentSession` or `linear.app/listAgentActivities`;
link mutations require `linear.app/updateAgentSession`. Put these grants under
`@submilli/linear-agent`, with the same team restriction. Main needs the matching
`linear-agent.<functionName>` grants. The wrapper does not need HTTP or secret
permissions, and main does not need direct access to `@submilli/linear`.

Linear resolves only the target's team metadata before its team check; a denied
team prevents the content read or mutation. Session IDs can also be restricted
through the `sessionId` field on the wrapper's capabilities.

Use the **owning app's OAuth token** as `LINEAR_API_KEY` (`actor=app`). Existing
Linear credential handling adds `Bearer` for OAuth tokens and preserves raw
personal API keys for ordinary Linear operations. Agent-session mutations
require the app that owns the session. No credentials are accepted in API calls.

## Development

```sh
node --test packages/linear-agent/scripts/contract.test.mjs
cargo run -p submilli -- build test --skip-network -p @submilli/linear-agent
```

Contract tests mock host functions and make no network calls. Native checks
compile the dependency, public API, and documentation examples. There are no
live Linear mutation tests in this package. Optional schema validation uses
`LINEAR_SCHEMA_PATH` and `GRAPHQL_MODULE_PATH`, as in the Linear package.

Caller-supplied activity UUIDv4 IDs are forwarded unchanged. Duplicate-ID
behavior is unverified; no idempotency or retry guarantee is made. Signals and
session states follow Linear's published schema, which may evolve independently.
The package never emits activities automatically or manages an agent loop.
