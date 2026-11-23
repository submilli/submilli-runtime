# @submilli/zendesk

A synchronous, typed client for the Zendesk Support API: tickets, public replies
and internal notes, attachments, users, organizations and groups, views and
macros, Help Center articles, satisfaction ratings and ticket metrics. Every
operation is checked, so a blueprint can limit an agent to some groups'
tickets, to notes rather than replies, to some fields, or to reads.

## Install

Choose the account when you install the package. `accountUrl` is the account's
`zendesk.com` URL, without a trailing slash (a host-mapped domain such as
`support.acme.com` is not an API host). It is compiled into the package, and
the package's grants name its host.

```sh
submilli install submilli/submilli-runtime @submilli/zendesk --config accountUrl=https://acme.zendesk.com
submilli server packages install submilli/submilli-runtime @submilli/zendesk --config accountUrl=https://acme.zendesk.com
```

A sandbox is a separate install with its own URL.

## Blueprint setup

The package reads one secret:

| Secret | Value |
| --- | --- |
| `ZENDESK_ACCESS_TOKEN` | An OAuth access token for the account, sent as `Authorization: Bearer` |

```yaml
packages:
  - "@submilli/zendesk"

secrets:
  ZENDESK_ACCESS_TOKEN:
    harness: ZENDESK_ACCESS_TOKEN
```

The harness binds the token of whoever the agent acts as: an agent's OAuth
token (authorization code with PKCE), so Zendesk applies that agent's role,
groups and ticket access, or a token from the client credentials grant. Create
the OAuth client in Admin Center (Apps and integrations, APIs, OAuth Clients)
with the `read` and `write` scopes, or `read` alone for a read-only agent.
Clients created since April 2026 issue tokens that expire, with a refresh token
the harness uses; an older client issues tokens that never expire, so create a
new one. API tokens (`email/token` basic auth) are not supported. The package
does not refresh tokens: an expired one fails with `ZendeskError`, status 401.

`submilli blueprint add-package @submilli/zendesk` writes the package's grants
with the account's host, for example:

```yaml
permissions:
  '@submilli/zendesk':
    - capability: http.get
      filter: host == "acme.zendesk.com"
      action: allow
    - capability: http.post
      filter: host == "acme.zendesk.com"
      action: allow
    - capability: http.put
      filter: host == "acme.zendesk.com"
      action: allow
    - capability: secrets.get
      filter: name == "ZENDESK_ACCESS_TOKEN"
      action: allow
```

`uploadAttachment` reads the VFS file it is given (`fs.read`, `fs.stat`).

## Two layers of permission

Zendesk decides what the token's user may see and change: their role, their
groups and their ticket access. The capabilities below limit the agent within
that, per operation. Facts a check needs that the caller does not supply (a
ticket's group, organization, brand, assignee, requester and status; a user's
role and organization) are read from Zendesk first and never taken from the
caller. A ticket that does not exist is checked with `""` for them, so a rule
such as `group == "360000123"` denies it as it denies another group's ticket,
and a denial does not reveal whether a ticket exists.

## Capabilities

Every check field is normalized before the check and sent exactly as checked:

- Ids are decimal strings without leading zeros, such as `"360000123"`. A
  ticket field the ticket leaves empty, such as an unassigned ticket's
  `assignee`, is `""`.
- `status` is a ticket's status, lowercase (`new`, `open`, `pending`, `hold`,
  `solved`, `closed`). On a write, `status` is the status it sets (`""` when it
  sets none) and `previousStatus` the ticket's before.
- `fields` is sorted and distinct: the fields a write sets, such as `status`,
  `priority`, `tags`, `due_at`, and `custom.<field id>` for custom fields;
  `user_fields.<key>` on users.
- A filter the caller leaves out is `""`, so a rule on it also refuses an
  unfiltered find. `assignee` is `"none"` when the filter asks for unassigned
  tickets.

| Capability | Operation | Fields |
| --- | --- | --- |
| `zendesk.com/tickets.get` | `getTicket`, each ticket of `viewTickets` | `group`, `organization`, `brand`, `assignee`, `requester`, `status` (looked up) |
| `zendesk.com/tickets.find` | `findTickets` | `group`, `organization`, `brand`, `assignee`, `requester`, `status` (a list), `tags` (as given) |
| `zendesk.com/tickets.count` | `countTickets` | as `tickets.find` |
| `zendesk.com/tickets.create` | `createTicket` | `group`, `organization`, `brand`, `assignee`, `requester` (as given, or `""`), `public` (the first comment's), `fields` |
| `zendesk.com/tickets.update` | `updateTicket` | the ticket's facts and `previousStatus` (looked up), `status`, `fields` |
| `zendesk.com/tickets.assign` | `assignTicket` | the ticket's facts (looked up), `toGroup`, `toAssignee` (`"none"` to unassign, `""` when unchanged) |
| `zendesk.com/tickets.merge` | `mergeTickets` | checked once for the target and once per source: `role` (`target` or `source`) and that ticket's facts (looked up) |
| `zendesk.com/tickets.metrics` | `ticketMetrics` | the ticket's facts (looked up) |
| `zendesk.com/comments.list` | `listComments` | the ticket's facts (looked up) |
| `zendesk.com/comments.note` | `addNote`, a macro's internal comment | the ticket's facts (looked up) |
| `zendesk.com/comments.reply` | `reply`, a public first comment, a macro's public comment | the ticket's facts and `previousStatus` (looked up), `status` |
| `zendesk.com/attachments.upload` | `uploadAttachment` | `path`, `fileName`, `contentType` |
| `zendesk.com/users.get` | `getUser` | `role`, `organization` (looked up) |
| `zendesk.com/users.find` | `findUsers` | `query` |
| `zendesk.com/users.create` | `createUser` | `organization`, `fields` |
| `zendesk.com/users.update` | `updateUser` | `role`, `organization` (looked up), `fields` |
| `zendesk.com/organizations.get` | `getOrganization` | `organization` |
| `zendesk.com/organizations.find` | `findOrganizations` | `name` |
| `zendesk.com/groups.list` | `listGroups` | none |
| `zendesk.com/groups.members` | `listAgents` | `group` |
| `zendesk.com/views.list` | `listViews` | none |
| `zendesk.com/views.read` | `viewTickets`, `countView` | `view` |
| `zendesk.com/macros.list` | `listMacros` | none |
| `zendesk.com/macros.preview` | `previewMacro` | `macro`, the ticket's facts (looked up) |
| `zendesk.com/macros.apply` | `applyMacro` | `macro`, the ticket's facts (looked up), `fields` (what the macro changes; `group` and `assignee` when it moves the ticket) |
| `zendesk.com/metadata.read` | `listTicketFields`, `listBrands` | `what` (`ticket_fields` or `brands`) |
| `zendesk.com/articles.search` | `searchArticles` | `query`, `locale`, `section` |
| `zendesk.com/articles.get` | `getArticle` | `section`, `locale` (looked up) |
| `zendesk.com/articles.browse` | `listCategories`, `listSections` | none |
| `zendesk.com/ratings.list` | `listSatisfactionRatings` | `score` |
| `zendesk.com/search.raw` | `search` | `query`, `type`: for administrators |

- `"looked up"` means read from Zendesk before the check.
- `updateTicket` never changes a ticket's group, assignee, requester,
  organization or brand, so a group-scoped rule cannot be bypassed by moving a
  ticket; `assignTicket` is the one way to move it, checked with where it is and
  where it goes. A macro that moves a ticket names `group` or `assignee` in
  `macros.apply`'s `fields`.
- A public comment emails the requester and everyone copied, so it is
  `comments.reply` wherever it comes from: `reply`, `createTicket` with a public
  first comment (checked in addition to `tickets.create`), and a macro's public
  comment (in addition to `macros.apply`). Ask a person before replies with
  `action: ask-human`.
- `viewTickets` checks `views.read` and then `tickets.get` for each ticket the
  view returns, so a view cannot reveal a ticket a rule would not let the agent
  read; a page with such a ticket is denied.
- `search` runs Zendesk's query language as written, so its scope cannot be
  checked: grant it only where a rule need not limit what the agent reads.
- A cursor carries no check of its own and is not tied to the call it came
  from: the next page is read under the check of the call it is passed to.
- Comment bodies, field values written, search text and page options are not
  check fields, except `findUsers`' and `search`'s query.

A blueprint for a first-line agent: group 100's tickets, notes freely, replies
after a person approves, status and tags but not priority, and moves only
within the group.

```yaml
permissions:
  main:
    - capability: zendesk.com/tickets.find
      filter: group == "100"
      action: allow
    - capability: zendesk.com/tickets.get
      filter: group == "100"
      action: allow
    - capability: zendesk.com/comments.list
      filter: group == "100"
      action: allow
    - capability: zendesk.com/comments.note
      filter: group == "100"
      action: allow
    - capability: zendesk.com/comments.reply
      filter: group == "100"
      action: ask-human
    - capability: zendesk.com/tickets.update
      filter: group == "100" and not (fields contains "priority")
      action: allow
    - capability: zendesk.com/tickets.assign
      filter: group == "100" and (toGroup == "" or toGroup == "100")
      action: allow
    - capability: zendesk.com/articles.search
      action: allow
```

## Write semantics

- Every ticket write (`updateTicket`, `assignTicket`, `addNote`, `reply`,
  `applyMacro`) sends Zendesk's `safe_update` with the `updated_at` the package
  read for its check. If the ticket changed in between, Zendesk refuses it with
  409 and the package throws `ZendeskError` `UpdateConflict` (`retryable`):
  read the ticket again and decide again.
- `addTags` and `removeTags` change only those tags: the package writes the
  ticket's tags as read for the check, plus and minus them, under the same
  `safe_update`, so a tag someone added meanwhile is never lost; `tags`
  replaces every tag.
- `applyMacro` reads what the macro would change (Zendesk's apply endpoint,
  which saves nothing), writes the fields that differ from the ticket and the
  comment it adds, in one conditional write.
- `mergeTickets` starts a Zendesk background job and returns it; the merge is
  done when the job is `completed`.
- Nothing is retried. `ZendeskError.retryable` marks HTTP 429, 503 and 409;
  `retryAfter` carries Zendesk's `Retry-After` seconds. A 5xx or a transport
  failure on a create may come after Zendesk applied it: look for the ticket
  before creating it again.
- A failure of the HTTP transport itself throws a plain `Error` from
  `submilli:http`.

## Search and pages

`findTickets` writes Zendesk's search query itself: ids, statuses, tags and
dates are validated, and `text` is a quoted phrase that may not contain a
double quote, so no program text changes the query's other terms. Search pages
by number and reads at most 1,000 results; `findTickets`, `findUsers`,
`findOrganizations` and `searchArticles` return `count` with each page. Lists
(comments, groups, agents, views, macros, ratings) use Zendesk's cursor
pagination. Help Center sections and categories return the first 100.

## Development

```sh
cargo run -p submilli -- build test -p @submilli/zendesk --skip-network --config accountUrl=https://example.zendesk.com
node --test packages/zendesk/scripts/contract.test.mjs
cargo test -p submilli --test package_policy
```

`tests/lib.test.ts` runs offline: id, tag, date and cursor validation, search
phrases, response and error mapping, and input validation before any check or
request.

`tests/policy/tier-one.ts` runs as a real `main` caller under a restricted
blueprint, without a token or network: finds scoped to a group and excluding
closed tickets, internal tickets only in that group and without a guarded
custom field, customers without notes, articles in one locale, the ticket
fields but not brands, bad ratings, one view, and raw search denied.

`zendesk_ticket_rules_hold_against_a_fake_zendesk` in
`crates/submilli/tests/package_policy.rs` covers what needs Zendesk's answers.
It publishes a copy of the package whose account URL points at a fake Zendesk on
`127.0.0.1` and runs `tests/fake-host/ticket-rules.ts`: the looked-up group
reaches each check, another group's ticket and a missing one are denied alike,
status and tags are allowed where priority is not, notes are allowed and
replies are not, a move within the group is allowed and to another group is
not, a macro that tags and notes is applied where one that replies or moves the
ticket is not, a customer is read but an agent is not, and a conflicting write
comes back as `UpdateConflict`. It asserts that only the allowed writes reach
the fake, each with `safe_update`.

`scripts/contract.test.mjs` runs the operations in Node against a mocked host
and a mocked Zendesk: the request each sends (method, path, query, body), what
its check sees after the lookups it makes first, that a denied check sends no
write, and how errors map.

Live tests run only with a token, never under `--skip-network`, against the
account the package is built for:

```sh
cargo run -p submilli -- build test -p @submilli/zendesk \
  --config accountUrl=https://<account>.zendesk.com --env-file <file>
```

- `tests/network_read.test.ts` needs `ZENDESK_ACCESS_TOKEN` and only reads.
- `tests/network_mutation.test.ts` also needs `ZENDESK_LIVE_MUTATIONS=true`. It
  creates a customer and tickets named with a run id, works them (notes, a
  reply to that customer, status, tags, assignment, a macro, a merge), and
  deletes them at the end, also when a step fails.
