# @submilli/linear

A synchronous, typed Linear GraphQL client for issues, comments, teams,
projects, and users.

## Blueprint setup

Add the installed package and declare the token as a required harness secret:

```yaml
packages:
  - "@submilli/linear"

secrets:
  LINEAR_API_KEY:
    harness:
      required: true
```

`submilli blueprint add-package @submilli/linear` adds the package and
scaffolds its permissions. Review them so the main program can call only the
`linear.app/*` operations it needs and the package can reach Linear and read
`LINEAR_API_KEY`.

Create a personal API key in Linear's API settings and bind it as
`LINEAR_API_KEY` when creating the harness session. The package reads the
secret directly; no `auth_proxy` rule is needed.

## Development

Run:

```bash
submilli build test -p @submilli/linear
```

Put `LINEAR_API_KEY` in the repository `.env` to enable the live read test.
