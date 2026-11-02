# @submilli/hacker-news

A read-only Hacker News client over the public HN Search API
(`hn.algolia.com`): search of stories and comments, and whole item threads.
The API needs no key, so the package reads no secret.

| Capability | Operation | Fields |
| --- | --- | --- |
| `news.ycombinator.com/search` | `search` | none |
| `news.ycombinator.com/items.get` | `getThread` | `itemId` |

## Blueprint setup

```yaml
packages:
  - "@submilli/hacker-news"
```

`submilli blueprint add-package @submilli/hacker-news --no-capabilities` adds
the package and its `http.get` rule for `hn.algolia.com`. Then grant `main`
the operations it needs:

```yaml
permissions:
  main:
    - capability: news.ycombinator.com/search
      action: allow
    - capability: news.ycombinator.com/items.get
      action: allow
```

## Behavior notes

- `search` defaults to newest first with exact word matching
  (`typoTolerance=false`, `queryType=prefixNone`), which is what a brand
  monitor wants. `exact: false` restores Algolia's fuzzy matching.
- `getThread` flattens the item tree depth-first with each parent before its
  children.
- The HN Search API indexes with a short delay, so a very recent item can be
  missing from search before it is in its thread.

## Development

```bash
submilli build test -p @submilli/hacker-news --skip-network
submilli build test -p @submilli/hacker-news
```

The network test reads the public API only.

## Policy tests

`tests/policy/scope.ts` runs as a real `main` caller under `scope.yaml`
without network: an `items.get` rule on `itemId` holds, and `search` stays
denied when only threads are granted. `cargo test -p submilli --test
package_policy` runs it.
