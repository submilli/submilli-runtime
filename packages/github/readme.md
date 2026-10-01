# @submilli/github

Native synchronous GitHub.com client for Submilli agents. The package exposes curated
repository, content, commit, issue, pull-request, team, release, and search operations
without leaking GitHub wire payloads.

## Setup

Bind a GitHub personal access token, OAuth token, or GitHub App installation token as
`GITHUB_TOKEN` in the blueprint:

```yaml
secrets:
  GITHUB_TOKEN: env:GITHUB_TOKEN
```

Grant only the package capabilities and HTTP requirements needed by the program. Fine-
grained token permissions still apply independently; read-only operations generally need
repository metadata/content access, while mutations need the matching Issues, Pull
requests, or Contents permission.

```ts
import { getRepository, listIssues, readTextFile } from "@submilli/github";

function main(): string {
    const repository = { owner: "octocat", name: "Hello-World" };
    const metadata = getRepository(repository);
    const readme = readTextFile(repository, "README");
    const issues = listIssues(repository, { limit: 10 });
    return JSON.stringify({
        repository: metadata,
        readme: readme === null ? null : readme.text,
        issues: issues.items,
    });
}
```

## Development

```sh
submilli build test -p @submilli/github
```

Live read tests require `GITHUB_TOKEN` and `GITHUB_TEST_REPOSITORY=owner/name`.
Mutation tests additionally require `GITHUB_LIVE_MUTATIONS=true` and must target a
disposable test repository.

## Policy tests

The TypeScript policy tests in `tests/policy/` run as a real `main` caller under
restricted blueprints. Each script has a blueprint of the same name:

- `owner-filter.ts` shows that a repository operation is allowed only where
  `owner == "allowed-org"`, and that `owner` and `name` are each read once even
  when `owner` answers differently on each read. Allowed cases must reach a
  deliberately denied credential boundary; denied cases must stop at the named
  business capability before reading credentials.
- `crafted-names.ts` shows that an owner or name holding search syntax, a
  slash, or a dot segment is rejected with `invalid_input` before the check,
  and that a scope qualifier in a search query is refused whatever whitespace
  precedes it.
- `ref-filters.ts` shows that file reads are held to a rule on `ref`, and pull
  request creation and retargeting to rules on `head` and `base`.
- `request-values.ts` shows that the request the package builds names the
  repository the policy approved. The package holds a placeholder token from
  `fake-token.txt`, and the blueprint refuses the request with a reason that
  tells the approved repository from any other.

These tests need no real token or network. Run from the repository root in an
isolated local package store:

```sh
github_test_home=$(mktemp -d)
SUBMILLI_HOME="$github_test_home" cargo run -p submilli -- build publish-local -p @submilli/github
SUBMILLI_HOME="$github_test_home" cargo run -p submilli -- run packages/github/tests/policy/owner-filter.ts --blueprint packages/github/tests/policy/owner-filter.yaml
SUBMILLI_HOME="$github_test_home" cargo run -p submilli -- run packages/github/tests/policy/request-values.ts --blueprint packages/github/tests/policy/request-values.yaml
```

These are separate commands because `build test` uses an unrestricted policy;
its ordinary unit tests cannot prove a `main` caller is constrained.
`cargo test -p submilli --test package_policy` runs them all.
