# `@submilli/salesforce`: design

A curated Package for Salesforce: the records, queries, collaboration, processes and reports an
agent works with, typed, and checked so that a Blueprint can limit what an agent reads and writes.
Tracked under SUB-1092 (CRM and BI coverage).

## What an agent can do with it

| Workflow | Operations |
| --- | --- |
| Brief me on an account | `getAccount`, `findContacts`, `findOpportunities`, `findCases`, `listActivities` |
| Review the pipeline | `findOpportunities`, `aggregate` (by stage, owner, close month) |
| Log a call and move the deal | `logActivity`, `updateOpportunity` |
| Triage support cases | `findCases`, `updateCase`, `postToFeed` |
| Clean up and convert leads | `findLeads`, `search`, `updateLead`, `convertLead` |
| Work with a custom object | `describeObject`, `query`, `getRecord`, `updateRecord` |
| Get a deal approved | `submitForApproval`, `listPendingApprovals` |
| Run the team's report | `listReports`, `runReport` |

## Credentials, the org's URL and hosts

The org is chosen when the Package is installed (Doron, 2026-11-19).

- `src/config.ts` declares `export declare const orgUrl: string;`, required, supplied with
  `--config orgUrl=https://acme.my.salesforce.com` on `submilli install` or
  `submilli server packages install`. It is compiled into the Package, and every request's URL
  is a top-level constant (`orgUrl + "/"`) followed by the path, so the compiler derives
  `host == "acme.my.salesforce.com"` into each `http.*` grant. A Blueprint limits the Package
  to that org with no runtime change. `apiVersion` keeps its default, `"68.0"` (Winter '27).
- The access token is the `SALESFORCE_ACCESS_TOKEN` secret, read inside the Package, as
  `@submilli/gmail` reads `GOOGLE_ACCESS_TOKEN`. The harness binds a user's OAuth token, so the
  agent acts as that user, or an integration user's token from the client credentials flow.
  The token must be for the installed org.
- One install serves one org; a sandbox is another install.
- The Package does not check the URL's form at run time: the admin chose it, and the grants
  show the host.
- The Package does not refresh tokens; an expired one surfaces as `INVALID_SESSION_ID`.

## Depends on

1. **Repository-wide builds use installed settings** (in this PR). `build check`, `build test`
   and `build publish-local` without `-p` gave every package empty settings, so they failed on
   `orgUrl` even after `build publish-local -p @submilli/salesforce --config …`, the retry their
   error message suggests; a `-p` build and every install already use installed settings
   (`crates/submilli-build/src/driver.rs`, `build_with_inputs`). Now repository-wide builds do
   too. CI (`packages.yml`), the Render image (`deploy-render.yml`, with
   `orgUrl=https://unconfigured.invalid` until the server reinstalls it with its org) and the
   tests install Salesforce first.
2. **Submilli Agents: Package settings.** Agents builds a Package with no settings
   (`crates/runtime/src/packages.rs`, `prepare_repo`). The Package resource needs a `config`
   map passed to `prepare_repo_configured` and included in the build cache key.
3. **Submilli Agents: an assumed token lifetime** on the OAuth provider, because Salesforce
   sends no `expires_in` (Doron agreed, 2026-11-19). Keeping `instance_url` is no longer needed.

## Two layers of permission

Salesforce applies its own: the token's user, with its profile, permission sets, sharing rules and
field-level security, sees and changes only what Salesforce lets them. The Package's checks limit
the agent within that, per operation, on the values below. Ownership facts the caller does not
supply (a record's owner and account) are read by the Package before the check, never taken from
the caller. A write checks the names of the fields it sets (`fields`, sorted and distinct, as
`@submilli/attio`'s `attributes`), so a rule can allow an opportunity's stage and not its amount.

## Operations

Capabilities are `salesforce.com/<group>.<operation>`. "Looked up" means the Package reads the
fact from Salesforce before the check.

### 1. Records, any object

| Operation | REST | Check fields |
| --- | --- | --- |
| `getRecord(object, id, {fields})` | `GET sobjects/{object}/{id}` | `object`, `ownerId` (in the record, when the object has one) |
| `createRecord(object, values)` | `POST sobjects/{object}` | `object`, `fields`, `ownerId` (given, or ""), `accountId` (given, or "") |
| `updateRecord(object, id, values)` | `PATCH sobjects/{object}/{id}` | `object`, `fields`, `ownerId` and `accountId` (looked up) |
| `deleteRecord(object, id)` | `DELETE sobjects/{object}/{id}` | `object`, `ownerId` and `accountId` (looked up) |
| `upsertRecord(object, externalIdField, value, values)` | `PATCH sobjects/{object}/{field}/{value}` | `object`, `externalIdField`, `fields` |
| `createRecords(object, records)`, `updateRecords(object, records)` | `composite/sobjects` (at most 200, all or none) | `object`, `fields` (union), `count` |
| `getRelated(object, id, relationship, page)` | `GET sobjects/{object}/{id}/{relationship}` | `object`, `relationship`, `ownerId` (looked up) |
| `getUpdated(object, start, end)`, `getDeleted(object, start, end)` | `sobjects/{object}/updated`, `/deleted` | `object` |

Values are `{ [field: string]: string | number | boolean | null }`; `null` clears a field.

### 2. Typed standard objects

`Account`, `Contact`, `Lead`, `Opportunity` (with `OpportunityLineItem`, `Product2`,
`Pricebook2`), `Case`, `Task`, `Event`, `User`, `Campaign` (with `CampaignMember`), `Contract`,
`Order`. Each has a typed interface after Salesforce's documented fields, `find<Object>s(filters,
page)` with typed filters (the SOQL is written inside the Package), and `get<Object>(id)`;
the common writes are typed too (`updateOpportunity`, `createCase`, `updateCase`, `createContact`,
`createLead`, `updateLead`). Every one takes `fields: string[]` for the org's custom fields
(`Region__c`), returned in `custom: { [field: string]: unknown }` beside the typed fields.

| Operation | Check fields |
| --- | --- |
| `findAccounts({name, ownerId, industry, type, modifiedAfter})` | `ownerId` |
| `findContacts({accountId, email, name})` | `accountId` |
| `findOpportunities({accountId, ownerId, stage, isClosed, closeAfter, closeBefore, minAmount})` | `accountId`, `ownerId` |
| `findCases({accountId, ownerId, status, priority, createdAfter})` | `accountId`, `ownerId` |
| `findLeads({ownerId, status, email, company})` | `ownerId` |
| `get…(id)` | `ownerId`, `accountId` (looked up) |
| `updateOpportunity(id, {stage, closeDate, amount, nextStep, …})` | `accountId`, `ownerId` (looked up), `fields`, `stage` |

A filter left out is "" in the check, so a rule such as `ownerId == ${vars.userId}` also refuses an
unfiltered search.

### 3. Queries and search

| Operation | REST | Check fields |
| --- | --- | --- |
| `query({object, fields, where, orderBy, limit}, page)` | `query` (SOQL written by the Package) | `object`, `fields` |
| `aggregate({object, groupBy, measures: count/sum/avg/min/max, where})` | `query` (`GROUP BY`) | `object`, `fields` |
| `search({text, objects, fields, limit})` | `parameterizedSearch` (SOSL) | `objects` |
| `exportQuery(query, path)` | Bulk API 2.0 query job, CSV written to the VFS | `object`, `fields`, `path` |
| `soql(query, page)` | `query` | none: its own capability, for admin Blueprints |
| `sosl(query)` | `search` | none: its own capability, for admin Blueprints |

`where` is a list of `{ field, op, value }` conditions combined with AND, values encoded by the
Package, so no caller text reaches SOQL unescaped. Pages always send an explicit size; nothing
loops inside the Package except `exportQuery`, which polls a bounded number of times.

### 4. Collaboration and activity

| Operation | REST | Check fields |
| --- | --- | --- |
| `logActivity({whatId, whoId, type: call/email/task, subject, description, date})` | `sobjects/Task` | `accountId` (looked up from `whatId`), `type` |
| `createEvent({…})` | `sobjects/Event` | `accountId` (looked up) |
| `listActivities(recordId, page)` | `ActivityHistories`, `OpenActivities` | `accountId` (looked up) |
| `createNote(recordId, title, body)` | `ContentNote` + `ContentDocumentLink` | `accountId` (looked up) |
| `uploadFile(recordId, path, {title})` | `ContentVersion` from a VFS file | `accountId` (looked up), `path` |
| `downloadFile(contentDocumentId, path)` | `ContentVersion/{id}/VersionData` to the VFS | `path` |
| `postToFeed(recordId, text, {mentions})` | Connect REST `chatter/feed-elements` | `accountId` (looked up), `mentions` |
| `readFeed(recordId, page)` | `chatter/feeds/record/{id}/feed-elements` | `accountId` (looked up) |
| `sendEmail({to, subject, body, relatedTo})` | `actions/standard/emailSimple` | `to` (domains), `accountId` (looked up) |

### 5. Process and automation

| Operation | REST | Check fields |
| --- | --- | --- |
| `submitForApproval(recordId, {comment, processName})` | `process/approvals` | `object`, `processName`, `accountId` (looked up) |
| `approve(workItemId, comment)`, `reject(workItemId, comment)` | `process/approvals` | `object`, `processName` (looked up) |
| `listPendingApprovals()` | `ProcessInstanceWorkitem` | none |
| `runFlow(name, inputs)` | `actions/custom/flow/{name}` | `flow` |
| `convertLead(leadId, {accountId, contactId, createOpportunity, opportunityName, convertedStatus, ownerId})` | SOAP `convertLead`, one XML request with the same token (Salesforce has no REST equivalent) | `ownerId` (looked up), `toOwnerId`, `accountId`, `createOpportunity` |
| `changeOwner(object, id, newOwnerId)` | `PATCH OwnerId` | `object`, `fromOwnerId` (looked up), `toOwnerId` |

`OwnerId` is refused in `updateRecord`, as Attio refuses status attributes in plain writes, so
rules on ownership cannot be bypassed.

### 6. Reports, dashboards and metadata

| Operation | REST | Check fields |
| --- | --- | --- |
| `listReports()`, `describeReport(id)` | `analytics/reports` | none, `reportId` |
| `runReport(id, {filters})` | `analytics/reports/{id}` | `reportId` |
| `listDashboards()`, `getDashboard(id)` | `analytics/dashboards` | none, `dashboardId` |
| `listListViews(object)`, `runListView(object, id, page)` | `sobjects/{object}/listviews` | `object` |
| `listObjects()`, `describeObject(object)` | `sobjects`, `sobjects/{object}/describe` | none, `object` |
| `getLimits()`, `getCurrentUser()` | `limits`, `chatter/users/me` | none |

`convertLead` works in every org with no setup, where a Flow would need each org's admin to build
one first. Its result names the account, contact and opportunity it created or used.

`runReport` returns what a model can use directly: the columns (label, API name, type), one flat
row per detail record with its group values filled in, and the group subtotals and grand totals as
a typed list. `{ raw: true }` returns Salesforce's own format (groupings and fact maps) instead.
Salesforce returns at most 2,000 detail rows per run; the result says when it was cut
(`truncated: true`), and `exportQuery` reads the full data.

## Errors and paging

- `SalesforceError` carries Salesforce's `errorCode` (such as `REQUIRED_FIELD_MISSING`,
  `INVALID_FIELD`, `REQUEST_LIMIT_EXCEEDED`), its `fields`, the HTTP status, and `retryable` for
  rate limits and `UNABLE_TO_LOCK_ROW`. Errors the Package raises itself have status 0, as in
  Attio.
- A missing record returns `null`. Batch writes report each record's errors.
- Pages are `{ records, totalSize, done, nextCursor }`. The next page is read by calling the same
  operation with the same arguments and `{ cursor }`, so it is checked as the first page was.

## Verification

- Offline: request builders (SOQL and SOSL escaping above all), response mapping, input
  validation before any request.
- Fake host (`tests/fake-host/`, run from `crates/submilli/tests/package_policy.rs` against a
  fake Salesforce on 127.0.0.1, as Attio's are): status and error mapping, paging, the
  looked-up owner and account reaching the check, and denied writes never reaching the fake.
  The Package is installed with `--config orgUrl=http://127.0.0.1:<port>`.
- Policy (`tests/policy/`): owner-scoped reads; a stage but not an amount; no owner changes
  except through `changeOwner`; `ask-human` on `sendEmail`; raw SOQL denied outside admin.
- Live (`network_read`, `network_mutation`): a Developer Edition org, through
  `--config orgUrl=…` and `SALESFORCE_ACCESS_TOKEN`. Writes create
  records marked with a run id and delete them.
- Contract: types against the org's generated OpenAPI 3.0 document (Salesforce's beta
  generator for the sObjects API).
- Era: reads against Era's larger simulated org; the Era benchmark's REST arm on this Package.

## Refined while building sections 1–3

- No `nextPage`: every paged operation takes `{ cursor }` and checks its own arguments again. A
  cursor is a validated query locator (`<id>-<offset>`), never a path.
- Values to write are `{ field, value }[]`, not an object: the read-once checker can verify a copy
  of an array, not of an object with dynamic keys.
- Check fields are lowercased API names (Salesforce is case-insensitive) and 18-character ids
  (case restored from the checksum), so a rule cannot be bypassed by spelling.
- A record that does not exist is checked with `""` for its owner and account, so a denial does not
  reveal whether it exists.
- Typed reads select `FIELDS(STANDARD)` plus their relationship names, so a field hidden by
  field-level security reads as null instead of failing the query.
- Typed updates also check the new and previous `stage`/`status`; batch writes check `ownerIds`
  and `accountIds` (looked up for updates); search checks `fields` as well as `objects`.
- `exportQuery` polls a bounded number of times and returns `done: false` with the job's id;
  calling again with `{ jobId }` continues it (the runtime has no sleep).
- The derived `http.*` grants name the installed org's host (`orgUrl`); `capabilities.yaml` in
  the repository shows `example.my.salesforce.com`, the setting the tests build with.

## Refined while building sections 4–6

- An operation on a record named only by its id (activities, notes, files, Chatter, approvals)
  finds the record's object from the id's prefix (a table of the common objects, else the org's
  object list) and checks `object` as well as the looked-up `ownerId` and `accountId`.
- `logActivity` takes the account from `whatId`, else from the `whoId` contact, and checks `type`,
  `object` and `fields`. `listActivities` reads `ActivityHistories` or `OpenActivities` as a
  subquery with Salesforce's required sort, at most 200 (`limit`), without a cursor.
- `readFeed` pages with Chatter's own page token (validated, 1 to 100 per page) and returns
  `{ posts, nextCursor }`, since Connect gives no total.
- `sendEmail` checks `to`, `domains` and `domain`, the one domain every recipient shares or `""`:
  the rule language has no "every element" test, so `domain == "example.com"` is how a
  blueprint allows only internal mail. At most 5 recipients. The `ask-human` rule is in the
  readme's example; `submilli run` has no person to ask, so the policy tests cover `domain`.
- `createNote` deletes the note when linking it to the record fails. `uploadFile` sends the file
  base64-encoded in JSON (at most 37 MB); `downloadFile` removes a download that answered with an
  error. `fs.remove` adds a `cannot derive filter for unknown parameter $recursive` warning that
  comes from the runtime's own annotation of `fs.remove`, not the package.
- `approve` and `reject` also check `accountId`; `listPendingApprovals` checks `actorId`;
  `runFlow` checks the input variables' names; `convertLead` also checks `contactId`, refuses a
  contact without its account, and defaults the converted status to the org's first;
  `changeOwner` also checks `accountId`.
- `runReport` with filters reads the report's describe and posts its metadata with the filters
  added (extending its boolean logic). Filters are not check fields. `{ raw: true }` fills a
  `raw` JSON text field beside the flat result instead of changing the return type.
- `listReports` and `listDashboards` query the `Report` and `Dashboard` objects, so they page and
  filter by name and folder; `analytics/reports` lists only the 200 most recently viewed.
- `runListView` runs the SOQL the view's describe gives, so its pages use query cursors.
- The fake-host test serves XML for the SOAP call and accepts the token in its session header.
- Checked against a Developer Edition org (2026-11-19, `network_read` and `network_mutation`):
  the activity subqueries, `ContentNote` and its link, file upload and download, the feed,
  SOAP `convertLead`, `getUpdated`/`getDeleted`, the Bulk export, a summary report's and a
  dashboard's JSON, and pending approvals. Not yet checked: the `emailSimple` inputs,
  `TargetObject.Type` on a real work item, and flows; the live test sends no email, submits no
  approval and runs no flow.

## Found by the live tests

- An ungrouped aggregate sent `LIMIT`, which Salesforce refuses; it now has none.
- `logActivity` wrote `Task.Type`, which orgs hide by default (field-level security), so every
  call failed. It now sets only `TaskSubtype`: `type` is `call`, `email` or `task`; a meeting
  is an event (`createEvent`, past times too). Typed task and event reads already used
  `FIELDS(STANDARD)`, so their `type` reads as null there.
- `createNote` failed with a bare 404 in an org with Notes turned off; it now says so.
- The standard library's `fs.remove`, `fs.mkdir`, `fs.list` and `isEmpty` tags bound a
  `$recursive` parameter that does not exist, so any package calling `remove` warned under
  `--deny-warnings`; the tags now type `recursive: boolean`.

## Decided

1. The org's URL is a required build setting chosen at install (above), not the token's
   `instance_url`; the grants then name the org's host with no runtime change.
2. `convertLead` is the SOAP call, so it works in every org without a Flow.
3. `runReport` returns flat rows and typed totals, with Salesforce's format on request.
