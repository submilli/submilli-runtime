---
title: "Install private packages on a server"
description: "How to let a server install packages from private GitHub repositories: create a token that can read them and give it to the server, under a process, Compose, or the Helm chart."
slug: server/install-private-packages
sidebar:
  order: 8
---

A server fetches packages from GitHub with a token of its own, never
the caller's, so out of the box it reaches public repositories only. A
package in a private repository needs the server to hold a token that
can read it. On your own machine the CLI uses your token instead, as
[Start a blueprint](/docs/blueprints/start-a-blueprint) shows.

This guide shows you how to let a server install private packages:
create a token that can read them, give it to the server under a
process, Compose, or the Helm chart, and read the errors when an
install fails. The example is Acme's billing package in
`acme/billing-package`; substitute your repository.

## Create the token

[Create a fine-grained
token](https://github.com/settings/personal-access-tokens/new?name=submilli&contents=read&expires_in=90)
on GitHub:

- **Resource owner:** the user or organization that owns the package
  repositories.
- **Repository access:** Only select repositories, the package
  repositories, including private ones they depend on.
- **Repository permissions:** Contents, Read-only. GitHub adds Metadata,
  Read-only, itself.

A fine-grained token covers one owner, and an organization may have to
approve it before it reads anything. It expires; choose an expiry you
will remember to renew. A classic token with the `repo` scope also works
and reaches every owner, but it can write to every repository you can;
in an organization with SAML single sign-on, authorize it with Configure
SSO.

## Give the server the token

Without one, an install from a private repository is refused with the
remedy:

```text
error: GitHub has no public repository `acme/billing-package`; if the repository is private, give the server a fine-grained personal access token with Repository permissions → Contents: Read-only on the package repositories (GitHub adds Metadata: Read-only), or a classic token with the `repo` scope, in the file named by `github_token_file` in its config file (create one at https://github.com/settings/personal-access-tokens/new?name=submilli&contents=read&expires_in=90&target_name=acme)
```

Put the token in a file only the server's user can read, and name it in
the config file. The setting exists only there, so the server's GitHub
credential is decided in one reviewable place:

```sh
(umask 077; printf '%s' "$GITHUB_TOKEN" > /etc/submilli/github.token)
```

```yaml title="server.yaml (fragment)"
github_token_file: /etc/submilli/github.token
```

The server refuses to start if the file is missing or empty:

```text
Error: `github_token_file`: reading the GitHub token file /etc/submilli/github.token: No such file or directory (os error 2)
```

It reads the file again on every install, so overwriting it in place
rotates the token without a restart. Then install as for a public
repository:

```sh
submilli server packages install acme/billing-package @acme/billing
```

```text
installed @acme/billing @ 990fa925b823
```

A repository the token can't read fails with the code `github_access`,
and GitHub's rate limit with `github_rate_limited`; the messages are in
the table below.

## Under Compose

Hand the server the token and a config file that names it, both as
files mounted the way the store key is on [Deploy with
Compose](/docs/server/deploy-with-compose), and with the same
`0444`, since the server runs as user 65532:

```sh
printf '%s' "$GITHUB_TOKEN" > github-token
printf 'github_token_file: /run/secrets/github-token\n' > server.yaml
chmod 0444 github-token server.yaml
```

```yaml title="compose.override.yaml (added to the same file)"
services:
  submilli:
    environment:
      SUBMILLI_CONFIG: /run/secrets/submilli-config
    secrets:
      - github-token
      - submilli-config

secrets:
  github-token:
    file: ./github-token
  submilli-config:
    file: ./server.yaml
```

To rotate the token, overwrite `github-token` in place, with `printf …
> github-token` as above. Compose mounts that one file, so an editor
that saves by replacing the file doesn't reach the container.

## Under the Helm chart

Put the token in a Secret and name it in `githubToken`:

```sh
kubectl create secret generic submilli-github --from-file=token=./github-token
```

```yaml title="values.yaml"
githubToken:
  existingSecret: submilli-github
```

The chart mounts it and sets `github_token_file`. Updating the Secret
rotates the token once the kubelet refreshes the mount; no restart is
needed.

## When it fails

| The error says | Do this |
| --- | --- |
| `GitHub has no public repository …` | Check the name; if the repository is private, set `github_token_file` |
| `GitHub has no repository … that the … token … can read` | Give the token Contents: Read-only on that repository, have the organization approve it, or check the name |
| `GitHub found no such branch, tag, or commit` | Check the ref given with `--sha` |
| `GitHub refused the token for …` | Give it Contents: Read-only there; otherwise the organization must approve it, or allows only another kind of token |
| `GitHub rejected the token (expired or revoked)` | Create a new one and replace the file |
| `uses SAML single sign-on: authorize …` | Open the link in the message, or Configure SSO on the token |
| `GitHub's rate limit is used up` (`github_rate_limited`) | Wait the time the message gives. Without a token the limit is far lower |
| `github_token_unavailable` | The server couldn't read `github_token_file` at install time; check the file and its permissions |
