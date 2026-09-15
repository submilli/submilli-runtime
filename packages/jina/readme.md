# @submilli/jina

Jina Reader and Search helpers for converting web pages and search results into
LLM-friendly markdown or structured data. Large results can be streamed
directly to the Submilli VFS.

## Blueprint setup

Add the installed package to the blueprint:

```yaml
packages:
  - "@submilli/jina"
```

`submilli blueprint add-package @submilli/jina` adds the package and scaffolds
its package permissions. Review the generated permissions and grant the main
program only the `jina.ai/read` and `jina.ai/search` operations it needs.

Jina supports anonymous requests at a lower rate limit. To use an API key,
declare an optional harness secret:

```yaml
secrets:
  JINA_API_KEY:
    harness:
      required: false
```

Bind `JINA_API_KEY` when creating the harness session. The package reads it
through `submilli:secrets`; no `auth_proxy` rule is needed.

## Development

Run the package tests from the repository root:

```bash
submilli build test -p @submilli/jina
```

Set `JINA_API_KEY` in `.env` to exercise authenticated network tests. Network
tests require outbound access to Jina's Reader and Search endpoints.
