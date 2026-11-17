# @submilli/linear-feedback

Files confirmed product feedback from an assistant as one issue in a configured
Linear team, in its triage state, or its first backlog state when the team has
no triage. See [docs/readme.md](docs/readme.md) for the API and how to collect
feedback well.

## Blueprint setup

```yaml
packages:
  - "@submilli/linear-feedback"

secrets:
  FEEDBACK_LINEAR_API_KEY:
    harness:
      required: true
  FEEDBACK_LINEAR_TEAM:
    harness:
      required: true
```

`FEEDBACK_LINEAR_API_KEY` is a Linear API key or app OAuth token that may create
issues in the feedback team. Prefer a key limited to that team with create
access only. `FEEDBACK_LINEAR_TEAM` is the team's key, such as `ENG`, or its UUID.
It is a secret so that a program cannot choose where feedback goes.

Allow the main program `linear.app/submitFeedback` and nothing else from Linear
when it should only file feedback. The capability carries `kind` and `test`, so
a rule can, for example, allow test filings only in a staging setup:

```yaml
permissions:
  main:
    - capability: linear.app/submitFeedback
      action: allow
```

## Development

```bash
submilli build test -p @submilli/linear-feedback
node --test packages/linear-feedback/scripts/contract.test.mjs
```

The contract test runs the real `submitFeedback` against a mocked host: no
network, no credential. Bind `FEEDBACK_LINEAR_API_KEY`,
`FEEDBACK_LINEAR_TEAM`, and `FEEDBACK_LINEAR_LIVE=true` to run the live test,
which files one `[TEST]` issue and prints its identifier; archive it afterwards.
