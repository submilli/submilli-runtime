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
