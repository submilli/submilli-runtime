# `@submilli/zendesk`: design

A curated Package for Zendesk Support: the tickets, conversations, people, queues, macros and Help
Center articles a support agent works with, typed, and checked so that a Blueprint can limit what
an agent reads, writes and says to customers. Tracked under SUB-2439. Follows
`@submilli/salesforce` (SUB-2438) and reuses its patterns: a required URL build setting, facts
looked up before checks, distinct capabilities for scope changes.

## What an agent can do with it

| Workflow | Operations |
| --- | --- |
| Triage the queue | `viewTickets`, `findTickets`, `updateTicket` (priority, type, tags, fields), `assignTicket` |
| Answer a ticket | `getTicket`, `listComments`, `addNote`, `reply`, `uploadAttachment` |
| Know the customer | `getUser`, `getOrganization`, `findTickets` (by requester or organization) |
| Draft from the knowledge base | `searchArticles`, `getArticle`, then `addNote` or `reply` |
| Apply the team's macro | `listMacros`, `previewMacro`, `applyMacro` |
| Open a ticket for a customer | `findUsers`, `createUser`, `createTicket` |
| Report on the queue | `countTickets`, `listSatisfactionRatings` |

## Credentials, the account's URL and hosts

- `src/config.ts` declares `export declare const accountUrl: string;`, required, supplied with
  `--config accountUrl=https://acme.zendesk.com` at install. As with Salesforce's `orgUrl`, it is
  compiled in and every URL is `ACCOUNT + path` (`const ACCOUNT = accountUrl + "/"`), so each
  `http.*` grant names `host == "acme.zendesk.com"`. A URL, not a subdomain, so fake-host tests
  can use `http://127.0.0.1:<port>`. A host-mapped domain (`support.acme.com`) is not an API host;
  the readme says to use the `zendesk.com` one.
- The access token is the `ZENDESK_ACCESS_TOKEN` secret, sent as `Authorization: Bearer`. The
  harness binds a user's OAuth token (authorization code with PKCE), so the agent acts as that
  agent seat, or a service token from the client credentials grant. No API tokens (basic auth with
  `email/token`): Zendesk is moving integrations to OAuth, and one auth scheme keeps the grants
  simple.
- Zendesk's token responses carry `expires_in` and a rotating refresh token for OAuth clients
  created since April 2026 ([refresh tokens](https://developer.zendesk.com/documentation/api-basics/authentication/refresh-token)),
  so Submilli Agents refreshes them as it does Google's. **No Agents change is needed.** A client
  created before then issues non-expiring tokens unless the token request passes `expires_in`;
  the readme says to create a new client.
- The Package does not refresh tokens; an expired one surfaces as a 401 `ZendeskError`.

## Two layers of permission

Zendesk applies its own: the token's user, with their role (agent, light agent, admin, custom
role), group memberships and ticket access ("tickets in their groups", "requested by their
organization", …) sees and changes only what Zendesk lets them. The Package's checks limit the
agent within that, per operation.

Facts the caller does not supply are read from Zendesk before the check, never taken from the
caller: a ticket's `group`, `organization`, `brand`, `assignee`, `requester` and `status`. A write
checks the names of the fields it sets (`fields`, sorted and distinct: system names such as
`priority`, and `custom.<field id>` for custom fields), so a rule can allow tags and priority and
not a custom "refund approved" field.

Ids are decimal strings (Zendesk's are integers below 2^53; strings keep them exact and match
other Packages' facts), validated as 1–16 digits before they reach a path or query.

## Operations

Capabilities are `zendesk.com/<group>.<operation>`. "Looked up" means read from Zendesk first.

### 1. Tickets

| Operation | Capability and facts |
| --- | --- |
| `getTicket(id)` | `tickets.get { group, organization, brand, assignee, requester, status }` looked up |
| `findTickets(filter, page?)` | `tickets.find { group?, organization?, requester?, assignee?, status?, brand? }` from the filter: typed filter (status set, group, assignee, requester, organization, brand, tags, created/updated ranges, free text), built into Zendesk search syntax with quoting; cursor paging through `/search/export` |
| `countTickets(filter)` | `tickets.count`, same facts as `find` |
| `createTicket(input)` | `tickets.create { group?, organization?, brand?, requester, fields }`; the first comment is public or private as asked, and a public one also needs `tickets.reply` |
| `updateTicket(id, changes)` | `tickets.update { …looked up, fields }`: status, priority, type, tags, subject, custom fields, due date. Refuses group, assignee, organization, requester and brand (`scope_field`) |
| `assignTicket(id, { group?, assignee? })` | `tickets.assign { …looked up, toGroup, toAssignee }` |
| `mergeTickets(target, sources, comments)` | `tickets.merge` on the target and each source, looked up |

Writes after a check send `safe_update: true` with `updated_stamp` set to the `updated_at` read
for the check, so a
ticket changed between the check and the write is refused (409) rather than written under a check
that saw other facts.

### 2. Conversation

| Operation | Capability and facts |
| --- | --- |
| `listComments(id, page?)` | `tickets.get` facts, capability `comments.list` |
| `addNote(id, body, attachments?)` | `comments.note`, ticket facts looked up |
| `reply(id, body, { status?, attachments? })` | `comments.reply`, ticket facts plus `requester`: a public comment emails the requester and CCs, so it is its own capability; example policies put `ask-human` on it |
| `uploadAttachment(name, contentType, bytes)` | `attachments.upload`; returns a token for `addNote`/`reply`/`createTicket` |

Bodies are plain text or Markdown-free HTML as Zendesk accepts them (`body` / `html_body`); the
Package sends `body` unless the caller asks for HTML.

### 3. People

| Operation | Capability and facts |
| --- | --- |
| `getUser(id)`, `findUsers(query)` | `users.get` / `users.find { role, organization }` looked up / from the result |
| `createUser(input)` | `users.create { role: "end-user", organization? }`: end users only; agents and admins are created in Zendesk |
| `updateUser(id, changes)` | `users.update { role, organization, fields }` looked up; refuses role changes |
| `getOrganization(id)`, `findOrganizations(query)` | `organizations.get` / `.find` |
| `listGroups()`, `listAgents(group?)` | `groups.list`, `groups.members { group }` |

### 4. Queues and macros

| Operation | Capability and facts |
| --- | --- |
| `listViews()`, `viewTickets(view, page?)`, `countView(view)` | `views.list`, `views.read { view }`. A view's tickets are checked as `tickets.get` each, so a rule scoped to groups still holds through a view |
| `listMacros(filter?)` | `macros.list` |
| `previewMacro(ticket, macro)` | `macros.preview { macro, …ticket facts }`: Zendesk's `apply` endpoint, which changes nothing |
| `applyMacro(ticket, macro)` | `macros.apply { macro, …ticket facts, fields }` and, when the macro adds a public comment, `comments.reply`; the preview's changes are written with `safe_update` |
| `listTicketFields()`, `listBrands()` | `metadata.read` |

### 5. Help Center

| Operation | Capability and facts |
| --- | --- |
| `searchArticles(query, { locale?, section? })` | `articles.search` |
| `getArticle(id)`, `listSections(category?)`, `listCategories()` | `articles.get { section, locale }` looked up |

Read-only: drafting or publishing articles is not in this Package.

### 6. Reporting

| Operation | Capability and facts |
| --- | --- |
| `listSatisfactionRatings(filter, page?)` | `ratings.list { score?, group? }` |
| `ticketMetrics(id)` | `tickets.get` facts: first reply, resolution and wait times |

Raw search in Zendesk's query language (`search(query)`, any object type) is offered as
`search.raw`, which example policies keep for admins, as Salesforce's raw SOQL is.

## Not in this Package

Side conversations (email third parties), ticket deletion and spam marking, triggers and
automations, user deletion and identities, Talk, Chat/Messaging (Sunshine Conversations), Guide
article writes, and incremental exports. Each can be its own capability group later.

## Errors and paging

- `ZendeskError` carries Zendesk's `error` (`RecordNotFound`, `RecordInvalid`, `Forbidden`, …),
  its `description`, field `details` for `RecordInvalid`, the HTTP status, and `retryable` with
  `retryAfter` from `Retry-After` on 429 and 503. A 409 from `safe_update` is `UpdateConflict`,
  retryable by reading again. Errors the Package raises have status 0.
- A missing record returns `null`.
- Pages are `{ items, nextCursor }` from Zendesk's cursor pagination (`page[size]`,
  `meta.after_cursor`). As in Salesforce, a cursor is read under the check of the call it is
  passed to. Offset pagination is not used (Zendesk caps it at 10,000 records and search at
  1,000).

## Verification

- Offline: search-query building and quoting, request builders, response mapping, id and input
  validation.
- Fake host (`tests/fake-host/`, from `crates/submilli/tests/package_policy.rs`, installed with
  `--config accountUrl=http://127.0.0.1:<port>`): looked-up facts reaching the check, denied
  writes and replies never reaching the fake, `safe_update` conflicts, 429 mapping, paging.
- Policy (`tests/policy/`): group-scoped reads; notes allowed, replies `ask-human`; no
  reassignment outside `assignTicket`; a custom field denied; raw search admin-only.
- Live (`network_read`, `network_mutation`): a Zendesk account through `ZENDESK_ACCESS_TOKEN`.
  Writes create tickets and end users marked with a run id, then delete them (deleting is an
  admin call made by the test, not a Package operation).
- Contract: `scripts/contract.test.mjs`, the exact requests and check contexts, as Salesforce's.
- Agents end to end: a local node with an OAuthProvider for the account (auth code with PKCE),
  two turns across a token refresh.

## Needed to start live tests

A Zendesk Suite trial (Team or higher, with Guide for Help Center) or a sponsored developer
account, with an admin user; an OAuth client (confidential, for client credentials in the live
tests; the same client's auth-code flow for the Agents run); `ZENDESK_ACCOUNT_URL`,
`ZENDESK_CLIENT_ID` and `ZENDESK_CLIENT_SECRET` in this worktree's `.env`.

## Refined while building

- **Search pages by number, not `/search/export`.** The export endpoint has no sort, and
  triage wants oldest-first. `findTickets`, `findUsers`, `findOrganizations` and
  `searchArticles` use Zendesk's numbered pages (cursor = the next page's number, refused past
  1,000 results) and return `count`. Lists (comments, groups, agents, views, macros, ratings)
  use cursor pagination.
- **Ids are 1–16 digits and a safe integer**, sent as JSON numbers; checks see the decimal
  text without leading zeros.
- **`addTags` and `removeTags` are options of `updateTicket`** (Zendesk's `additional_tags` and
  `remove_tags`, sent with `safe_update`), not separate operations; `tags` replaces all and
  cannot be combined with them.
- **No `getAttachment`.** Attachments are served from another host
  (`*.zdusercontent.com`), which the account-URL grant does not name. Comments return each
  attachment's `contentUrl`; downloading waits for a second build setting or a runtime change.
- **`applyMacro` diffs the preview against the ticket** for a fixed list of writable fields
  (subject, status, priority, type, tags, group, assignee, due date, problem, custom fields),
  so it works whether Zendesk's apply endpoint returns the whole ticket or only the changes.
  The check's `fields` names `group`/`assignee` when the macro moves the ticket, and its
  comment is checked as `comments.reply` or `comments.note`.
- **`createTicket` with a public first comment** checks `tickets.create` (with `public`) and
  then `comments.reply`, so every customer-visible message passes the reply rule.
- **`viewTickets` checks `views.read`, then `tickets.get` for each ticket**; a page holding a
  ticket a rule would not let the agent read is denied.
- **`users.find` checks the `query`**; `organizations.find` the `name`. `userFields` on writes
  is a list of `{ key, value }`, since the check discipline refuses iterating a caller's record
  before `check()`.
- **Enum inputs** (status, priority, type, sort, score) are type-checked by the runtime before
  the package sees them; the package's own validation also covers values built at run time.
- The fake-host helper `copy_package` in `package_policy.rs` now serves Salesforce and Zendesk.
