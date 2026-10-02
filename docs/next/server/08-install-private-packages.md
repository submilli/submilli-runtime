---
title: "Install private packages"
description: "How to give the CLI and a server a GitHub token, so they install packages from private GitHub repositories."
slug: next/server/install-private-packages
pagefind: false
sidebar:
  order: 8
  hidden: true
---

Packages install from GitHub, and a private repository needs a GitHub token
that can read it. The CLI uses yours; a server uses its own, never the
caller's.

## Create the token

[Create a fine-grained
token](https://github.com/settings/personal-access-tokens/new?name=submilli&contents=read&expires_in=90)
on GitHub:

- **Resource owner:** the user or organization that owns the package
  repositories.
- **Repository access:** Only select repositories, the package repositories,
  including private ones they depend on.
- **Repository permissions:** Contents → Read-only. GitHub adds Metadata →
  Read-only itself.

A fine-grained token covers one owner. An organization may have to approve
it before it works. A classic token with the `repo` scope also works, and
can reach several owners, but it can write to every repository you can; in
an organization with SAML single sign-on, authorize it with Configure SSO.

Fine-grained tokens expire. Choose an expiry you will remember to renew.

## On your machine

```sh
submilli github authenticate
```

It asks for the token, checks it with GitHub, and stores it:

```text
✓ stored a GitHub token for octocat (expires 2026-12-31 00:00:00 UTC) in /home/you/.submilli/github_token
```

`submilli install` and `submilli build` send it from then on. `GH_TOKEN` or
`GITHUB_TOKEN` wins over it, which is how CI supplies one, and with none of
them the GitHub CLI's token (`gh auth token`) is used. `submilli github
auth-status` shows which applies.

## On a server

Put the token in a file and name it in the server's config file:

```yaml title="server.yaml"
github_token_file: /etc/submilli/github.token
```

With Docker Compose or Kubernetes, mount it from a secret instead:
[deploying](/docs/deploying) covers both. The server refuses to start if the
file is missing or empty, and reads it on every install, so replacing the
file rotates the token without a restart.

```sh
submilli server packages install acme/billing-package @acme/billing
```

## When it fails

| Error | Fix |
| --- | --- |
| no public repository `org/repo`, or no public commit in it | Check the name and commit; if the repository is private, authenticate, or set `github_token_file`. |
| no repository `org/repo`, or no commit in it, that the token can read | Give the token Contents: Read-only on that repository, have the organization approve it, or check the name. |
| no such branch, tag, or commit in `org/repo` | Check the ref after `@`. |
| GitHub refused the token for `org/repo` | Give it Contents: Read-only there; otherwise the organization must approve it or allows only another kind of token. |
| GitHub rejected the token (expired or revoked) | Create a new one. |
| uses SAML single sign-on: authorize … for it | Open the link in the message, or Configure SSO on the token. |
| GitHub won't identify this token (from `submilli github authenticate`) | It is a GitHub App or Actions token: set it in `GH_TOKEN` or `GITHUB_TOKEN` instead. |
| GitHub's rate limit is used up (`github_rate_limited` on a server) | Wait the time the message gives. Without a token the limit is far lower. |
| `github_token_unavailable` (server) | The server couldn't read `github_token_file` at install time. |
