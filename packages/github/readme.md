# @submilli/github

Native synchronous GitHub.com client for Submilli agents. The package exposes curated
repository, content, commit, issue, pull-request, team, release, search, and public
profile operations without leaking GitHub wire payloads.

## Setup

Bind a GitHub personal access token, OAuth token, or GitHub App installation token as
`GITHUB_TOKEN` in the blueprint:

```yaml
secrets:
  GITHUB_TOKEN: { store: GITHUB_TOKEN }
```

Populate the key with `submilli secret put GITHUB_TOKEN` for local runs or
`submilli server secret put GITHUB_TOKEN` for a server.

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
- `search-scope.ts` checks all three search operations: visibility/state qualifiers
  and quoted phrases pass, while scope qualifiers outside phrases are refused even
  beside punctuation or format characters. Validation folds ASCII case and fullwidth
  ASCII; requests retain the original query. Issue/PR kind qualifiers (including
  `type:` aliases and quoted values) and OR are refused. Unterminated and escaped quotes are refused
  because the search endpoints differ in their escape syntax.
- `ref-filters.ts` shows that file reads are held to a rule on `ref`, and pull
  request creation and retargeting to rules on `head` and `base`, and that a
  declared field an operation does not name is checked as null.
- `request-values.ts` shows that the request the package builds names the
  repository the policy approved. The package reads a placeholder token from
  the local secret store, and the blueprint refuses the request with a reason that
  tells the approved repository from any other.
- `people-scope.ts` shows that events, stargazers and forks are held to a rule on
  `owner` and `repo`, and the profile reads to a rule on `login`, and that a crafted
  login is rejected with `invalid_input` even under a prefix rule it would match.
- `people-requests.ts` shows the same operations request the exact lowercased path
  that was checked, by the method in `request-values.ts`.

These tests need no real token or network. Run from the repository root in an
isolated local package store:

```sh
github_test_home=$(mktemp -d)
SUBMILLI_HOME="$github_test_home" cargo run -p submilli -- build publish-local -p @submilli/github
SUBMILLI_HOME="$github_test_home" cargo run -p submilli -- run packages/github/tests/policy/owner-filter.ts --blueprint packages/github/tests/policy/owner-filter.yaml
printf %s policy-placeholder | SUBMILLI_HOME="$github_test_home" cargo run -p submilli -- secret put policy-token
SUBMILLI_HOME="$github_test_home" cargo run -p submilli -- run packages/github/tests/policy/request-values.ts --blueprint packages/github/tests/policy/request-values.yaml
```

These are separate commands because `build test` uses an unrestricted policy;
its ordinary unit tests cannot prove a `main` caller is constrained.
`cargo test -p submilli --test package_policy` runs them all.

## Capability compatibility notes

Repository `owner` and `repo` identities are now lowercase in capability checks,
REST paths, GraphQL variables, and scoped search qualifiers. Update mixed-case
blueprint literals to lowercase. GitHub repository names are case-insensitive;
branch/ref and file-path values retain their case. Prefer allow rules for refs:
`main`, `heads/main`, tags and commit SHAs can name the same history.
`listCommits` checks its `sha` option as `ref` and its path as `path`; omitted or
empty values are null and are not sent.

## Audience and profile reads

These read-only operations show who starred or forked a repository and what an account
publishes about itself. Each has its own grant:

| Function | Capability | Filter fields |
| --- | --- | --- |
| `listRepositoryEvents` | `github.com/events.list` | `owner`, `repo` |
| `listStargazers` | `github.com/stargazers.list` | `owner`, `repo` |
| `listForks` | `github.com/forks.list` | `owner`, `repo` |
| `getUser` | `github.com/users.get` | `login` |
| `listUserOrganizations` | `github.com/userOrganizations.list` | `login` |
| `listUserSocialAccounts` | `github.com/userSocialAccounts.list` | `login` |

The three repository capabilities declare the same fields as `github.com/repositories.get`;
the fields they do not use are null. `login` is lowercase. It must be GitHub's login shape
(letters, digits and single hyphens, neither first nor last, at most 39 characters, with an
optional `[bot]` suffix), so an older login outside it, such as one ending in a hyphen, is
refused with `invalid_input`.

`getUser` never returns an email address. GitHub's `email` becomes the boolean
`hasPublicEmail`, so granting a profile read does not grant personal contact data, and there
is no raw passthrough. Free-text fields such as `bio` and `blog` are returned as the account
wrote them, and can hold whatever it chose to publish there. These operations do not search
by name or email or read commits. The keeping of addresses out of an agent's reach also depends
on its other grants: `github.com/commits.list` and `github.com/commits.get` return
`authorEmail`, and `github.com/users.search` takes a query that can match on an address.

Unlike the single-resource reads, which return `null` when missing, `getUser` and these list
calls throw `GitHubError`: code `not_found` for a missing repository or login, and `forbidden`
when GitHub refuses. GitHub restricts `/stargazers` to a repository's admins and collaborators,
so read stars from `listRepositoryEvents` when `listStargazers` is refused. The events feed
covers recent activity only. `listUserOrganizations` returns public memberships only.

## Research search

`searchIssuesAcrossRepositories` and `searchPullRequestsAcrossRepositories`
require their own broad grants (`github.com/issues.searchAcrossRepositories` and
`github.com/pulls.searchAcrossRepositories`). Existing repository grants do not
permit them. They search repositories visible to the token; raw `repo:`, `org:`
and `user:` qualifiers can narrow discovery. Their checks include the final
`query`, normalized `author`, and inclusive `createdSince`/`createdUntil` dates.
Use one login or `app/login` and UTC calendar dates in `YYYY-MM-DD` form. Kind,
author, creation qualifiers and unquoted `OR` in the query are rejected so they
cannot override those structured options. Quoted phrases remain literal terms.

`searchPullRequestSummaries` is repository-restricted and needs
`github.com/pulls.searchSummaries`. It uses the same scope guard as existing
search. Both summary APIs make one request per page, without per-hit detail calls.
Existing `searchPullRequests` keeps its full-detail contract. Enrich selected
summary hits with `getPullRequest`; use `getPullRequestReactions` (grant `github.com/pulls.getReactions`) for
issue-style reaction counts when the pull-request detail endpoint omits them.

Research pages retain `totalCount`, `nextPageToken`, and `incompleteResults`.
`isComplete` means there is no next accessible page; it does not mean the search
is exhaustive. `isCapped` reports more than 1,000 matches. Narrow or partition a
capped query by date/repository, and check `incompleteResults` even on its last
page. GitHub also limits the search scope to 4,000 repositories and can time out.
REST search uses a separate rate-limit bucket (30 authenticated requests/minute,
10 unauthenticated); requests are not retried. Respect typed rate-limit metadata.
GitHub may return validation errors for query length/boolean complexity. These
limits are documented in [REST search](https://docs.github.com/en/rest/search/search)
and [issue/PR qualifiers](https://docs.github.com/en/search-github/searching-on-github/searching-issues-and-pull-requests).

Issue, pull-request and comment models expose nullable `reactions`. A summary or
individual counter omitted by the endpoint is null; an explicit zero is zero.
GraphQL issue lists currently return null reaction summaries. Search hits carry
repository identity, URLs, authors and creation/update timestamps; PR summaries
omit mergeability, diff statistics, and head/base details rather than fabricating
those values.

Offline request-contract checks:

```sh
node --test packages/github/scripts/contract.test.mjs
```
