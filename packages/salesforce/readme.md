# @submilli/salesforce

A synchronous, typed client for the Salesforce REST API: records of any
object, typed standard objects (accounts, contacts, leads, opportunities and
their products, cases, tasks, events, users, campaigns, contracts, orders),
queries written from typed conditions, aggregates, full-text search, Bulk API
exports, activities, notes, files, Chatter and email, approvals, flows, lead
conversion and ownership changes, reports, dashboards, list views and metadata.
Every operation is checked, so a blueprint can limit an agent to its own
records, to some objects or fields, or to reads.

## Install

Choose the org when you install the package. `orgUrl` is the org's My Domain
URL, without a trailing slash; it is compiled into the package, and the
package's grants name its host.

```sh
submilli install submilli/submilli-runtime @submilli/salesforce --config orgUrl=https://acme.my.salesforce.com
submilli server packages install submilli/submilli-runtime @submilli/salesforce --config orgUrl=https://acme.my.salesforce.com
```

A sandbox is a separate install with its own URL, such as
`https://acme--uat.sandbox.my.salesforce.com`. `apiVersion` defaults to `68.0`
(Winter '27).

## Blueprint setup

The package reads one secret:

| Secret | Value |
| --- | --- |
| `SALESFORCE_ACCESS_TOKEN` | An access token for the org, sent as `Authorization: Bearer` |

```yaml
packages:
  - "@submilli/salesforce"

secrets:
  SALESFORCE_ACCESS_TOKEN:
    harness: SALESFORCE_ACCESS_TOKEN
```

The harness binds the token of whoever the agent acts as: a user's OAuth token,
so Salesforce applies that user's profile, permission sets, sharing rules and
field-level security, or an integration user's token from an External Client
App's client credentials flow (the app's **Run As** user). The token needs the
`api` scope and must be for the org the package was installed with. The
package does not refresh tokens: an expired one fails with `INVALID_SESSION_ID`.

`submilli blueprint add-package @submilli/salesforce` writes the package's
grants with the org's host, for example:

```yaml
permissions:
  '@submilli/salesforce':
    - capability: http.get
      filter: host == "acme.my.salesforce.com"
      action: allow
    - capability: http.post
      filter: host == "acme.my.salesforce.com"
      action: allow
    - capability: http.patch
      filter: host == "acme.my.salesforce.com"
      action: allow
    - capability: http.delete
      filter: host == "acme.my.salesforce.com"
      action: allow
    - capability: http.download
      filter: host == "acme.my.salesforce.com"
      action: allow
    - capability: secrets.get
      filter: name == "SALESFORCE_ACCESS_TOKEN"
      action: allow
```

The package writes and reads the VFS for `exportQuery` and `downloadFile` (`fs.write` on
the path given) and `uploadFile` (`fs.read` and `fs.stat`), and removes a
download that failed (`fs.remove`). `convertLead` uses the SOAP API
(`/services/Soap/u/68.0`) on the same host, because Salesforce has no REST
equivalent; it carries the token in the SOAP session header.

## Two layers of permission

Salesforce decides what the token's user may see and change. The capabilities
below limit the agent within that, per operation. Facts a check needs that the
caller does not supply, a record's owner and account, are read from Salesforce
first and never taken from the caller. A record that does not exist is checked
with `""` for them, so a rule such as `ownerId == "005…"` denies it as it
denies another owner's record, and a denial does not reveal whether a record
exists.

## Capabilities

Every check field is normalized before the check and sent exactly as checked:

- `object`, `fields`, `objects`, `relationship` and `externalIdField` are
  lowercased API names (Salesforce treats `Amount` and `amount` as the same
  field), so a rule compares `object == "opportunity"`. A parent's field is a
  path, such as `account.name`.
- Ids are 18-character Salesforce ids: a 15-character id, or an 18-character
  id in another case, is checked in its 18-character form.
- `fields` is sorted and distinct. On a write it is the fields the request
  sets; on a query, every field it reads, filters on or sorts by.
- A filter the caller leaves out is `""`, so a rule on it also refuses an
  unfiltered find.

| Capability | Operation | Fields |
| --- | --- | --- |
| `salesforce.com/records.get` | `getRecord` | `object`, `ownerId` and `accountId` (looked up) |
| `salesforce.com/records.create` | `createRecord` | `object`, `fields`, `ownerId` and `accountId` (as given, or `""`) |
| `salesforce.com/records.update` | `updateRecord` | `object`, `fields`, `ownerId` and `accountId` (looked up) |
| `salesforce.com/records.delete` | `deleteRecord` | `object`, `ownerId` and `accountId` (looked up) |
| `salesforce.com/records.upsert` | `upsertRecord` | `object`, `externalIdField`, `externalId`, `fields` |
| `salesforce.com/records.createMany` | `createRecords` | `object`, `fields` (all records'), `count`, `ownerIds` and `accountIds` (as given) |
| `salesforce.com/records.updateMany` | `updateRecords` | `object`, `fields`, `count`, `ownerIds` and `accountIds` (looked up) |
| `salesforce.com/records.related` | `getRelated` | `object`, `relationship`, `ownerId` and `accountId` of the parent (looked up) |
| `salesforce.com/records.updated` | `getUpdated` | `object` |
| `salesforce.com/records.deleted` | `getDeleted` | `object` |
| `salesforce.com/accounts.find` | `findAccounts` | `ownerId` |
| `salesforce.com/accounts.get` | `getAccount` | `ownerId` (looked up), `accountId` (the id) |
| `salesforce.com/contacts.find` | `findContacts` | `accountId` |
| `salesforce.com/contacts.get` | `getContact` | `ownerId`, `accountId` (looked up) |
| `salesforce.com/contacts.create` | `createContact` | `accountId`, `ownerId`, `fields` |
| `salesforce.com/leads.find` | `findLeads` | `ownerId` |
| `salesforce.com/leads.get` | `getLead` | `ownerId` (looked up) |
| `salesforce.com/leads.create` | `createLead` | `ownerId`, `status`, `fields` |
| `salesforce.com/leads.update` | `updateLead` | `ownerId` and `previousStatus` (looked up), `status`, `fields` |
| `salesforce.com/opportunities.find` | `findOpportunities` | `accountId`, `ownerId` |
| `salesforce.com/opportunities.get` | `getOpportunity` | `ownerId`, `accountId` (looked up) |
| `salesforce.com/opportunities.update` | `updateOpportunity` | `accountId`, `ownerId` and `previousStage` (looked up), `stage`, `fields` |
| `salesforce.com/opportunityLineItems.find` | `findOpportunityLineItems` | `opportunityId` |
| `salesforce.com/opportunityLineItems.get` | `getOpportunityLineItem` | `opportunityId` (looked up) |
| `salesforce.com/products.find` | `findProducts` | none |
| `salesforce.com/products.get` | `getProduct` | none |
| `salesforce.com/pricebooks.find` | `findPricebooks` | none |
| `salesforce.com/pricebooks.get` | `getPricebook` | none |
| `salesforce.com/cases.find` | `findCases` | `accountId`, `ownerId` |
| `salesforce.com/cases.get` | `getCase` | `ownerId`, `accountId` (looked up) |
| `salesforce.com/cases.create` | `createCase` | `accountId`, `ownerId`, `fields` |
| `salesforce.com/cases.update` | `updateCase` | `accountId`, `ownerId` and `previousStatus` (looked up), `status`, `fields` |
| `salesforce.com/tasks.find` | `findTasks` | `ownerId`, `whatId` |
| `salesforce.com/tasks.get` | `getTask` | `ownerId`, `accountId` (looked up) |
| `salesforce.com/events.find` | `findEvents` | `ownerId`, `whatId` |
| `salesforce.com/events.get` | `getEvent` | `ownerId`, `accountId` (looked up) |
| `salesforce.com/users.find` | `findUsers` | none |
| `salesforce.com/users.get` | `getUser` | none |
| `salesforce.com/campaigns.find` | `findCampaigns` | `ownerId` |
| `salesforce.com/campaigns.get` | `getCampaign` | `ownerId` (looked up) |
| `salesforce.com/campaignMembers.find` | `findCampaignMembers` | `campaignId` |
| `salesforce.com/campaignMembers.get` | `getCampaignMember` | `campaignId` (looked up) |
| `salesforce.com/contracts.find` | `findContracts` | `accountId`, `ownerId` |
| `salesforce.com/contracts.get` | `getContract` | `ownerId`, `accountId` (looked up) |
| `salesforce.com/orders.find` | `findOrders` | `accountId`, `ownerId` |
| `salesforce.com/orders.get` | `getOrder` | `ownerId`, `accountId` (looked up) |
| `salesforce.com/query.run` | `query` | `object`, `fields` |
| `salesforce.com/query.aggregate` | `aggregate` | `object`, `fields` |
| `salesforce.com/query.search` | `search` | `objects`, `fields` |
| `salesforce.com/query.export` | `exportQuery` | `object`, `fields`, `path` |
| `salesforce.com/query.soql` | `soql` | none: for administrators |
| `salesforce.com/query.sosl` | `sosl` | none: for administrators |
| `salesforce.com/activities.log` | `logActivity` | `type`, `object` of `whatId`, `accountId` (looked up from `whatId`, else from the `whoId` contact), `fields` |
| `salesforce.com/activities.createEvent` | `createEvent` | `object`, `accountId` (looked up as above), `fields` |
| `salesforce.com/activities.list` | `listActivities` | `object`, `accountId`, `ownerId` (looked up) |
| `salesforce.com/notes.create` | `createNote` | `object`, `accountId`, `ownerId` (looked up) |
| `salesforce.com/files.upload` | `uploadFile` | `object`, `accountId`, `ownerId` (looked up), `path` |
| `salesforce.com/files.download` | `downloadFile` | `path` |
| `salesforce.com/feed.post` | `postToFeed` | `object`, `accountId`, `ownerId` (looked up), `mentions` |
| `salesforce.com/feed.read` | `readFeed` | `object`, `accountId`, `ownerId` (looked up) |
| `salesforce.com/email.send` | `sendEmail` | `to`, `domains`, `domain` (the one domain every recipient shares, or `""`), `object` and `accountId` of `relatedTo` (looked up) |
| `salesforce.com/approvals.submit` | `submitForApproval` | `object`, `processName` (as given, or `""`), `accountId`, `ownerId` (looked up) |
| `salesforce.com/approvals.approve` | `approve` | `object`, `processName`, `accountId` (looked up from the work item) |
| `salesforce.com/approvals.reject` | `reject` | `object`, `processName`, `accountId` (looked up from the work item) |
| `salesforce.com/approvals.listPending` | `listPendingApprovals` | `actorId` |
| `salesforce.com/flows.run` | `runFlow` | `flow`, `inputs` (the variables' names) |
| `salesforce.com/leads.convert` | `convertLead` | `ownerId` (the lead's, looked up), `toOwnerId`, `accountId`, `contactId` (as given, or `""`), `createOpportunity` |
| `salesforce.com/records.changeOwner` | `changeOwner` | `object`, `fromOwnerId` and `accountId` (looked up), `toOwnerId` |
| `salesforce.com/reports.list` | `listReports` | none |
| `salesforce.com/reports.describe` | `describeReport` | `reportId` |
| `salesforce.com/reports.run` | `runReport` | `reportId` |
| `salesforce.com/dashboards.list` | `listDashboards` | none |
| `salesforce.com/dashboards.get` | `getDashboard` | `dashboardId` |
| `salesforce.com/listViews.list` | `listListViews` | `object` |
| `salesforce.com/listViews.run` | `runListView` | `object`, `listViewId` |
| `salesforce.com/objects.list` | `listObjects` | none |
| `salesforce.com/objects.describe` | `describeObject` | `object` |
| `salesforce.com/org.limits` | `getLimits` | none |
| `salesforce.com/org.currentUser` | `getCurrentUser` | none |

- `"looked up"` means read from Salesforce before the check. An account's
  `accountId` is its own id; an object without an owner or account field
  checks `""`.
- `OwnerId` is refused in every update (`updateRecord`, `updateRecords`,
  `upsertRecord` and the typed updates), so an owner-scoped rule cannot be
  bypassed by reassigning a record; `changeOwner` is the one way to reassign,
  checked with the owner before and after. A create may set it, and the check
  sees it.
- Operations on a record named only by its id (activities, notes, files,
  Chatter, approvals) find its object from the id's prefix, and check `object`
  as a lowercased API name, `""` when no object has that prefix.
- `object` names the record, never a field: `fields` on `logActivity`,
  `createEvent` and the typed writes are the task's or event's own fields,
  including the ones the operation sets (`TaskSubtype`, `Type`, `Status`).
- `flow`, `processName` and `inputs` are lowercased API names.
- `updateRecords` and `createRecords` check every record's owner and account
  together, as sorted, distinct lists. A rule that every record is the
  caller's: `ownerIds.0 == "005…" and not (ownerIds.1 glob "*")`.
- `soql` and `sosl` run text as written, so their checks have no fields: grant
  them only where a rule need not limit what the agent reads.
- A cursor carries no check of its own: the next page is checked with the
  arguments the program passes with it, and the program must pass the same
  ones. A cursor is a Salesforce query locator, valid for the token's user and
  only for a while.
- Some reads follow a passed check without one of their own: the owner and
  account lookups read the record, and a typed update reads its current stage
  or status. Their values reach only the check.
- Values written, condition values, report filters, email subjects and bodies,
  flow input values and page options are not check fields. A rule that should
  depend on a report's filters belongs on `query` or `aggregate` instead.
- The rule language has no "every element" test, so `sendEmail` also checks
  `domain`: `domain == "example.com"` allows mail only when every recipient is
  at that domain. Ask a person before other mail with `action: ask-human`.

A blueprint for a sales rep's agent: its own pipeline, stage changes but not
amounts on its own deals, and tasks.

```yaml
permissions:
  main:
    - capability: salesforce.com/opportunities.find
      filter: ownerId == "005000000000001AAA"
      action: allow
    - capability: salesforce.com/opportunities.get
      filter: ownerId == "005000000000001AAA"
      action: allow
    - capability: salesforce.com/opportunities.update
      filter: ownerId == "005000000000001AAA" and not (fields contains "amount")
      action: allow
    - capability: salesforce.com/query.aggregate
      filter: object == "opportunity"
      action: allow
    - capability: salesforce.com/records.create
      filter: object == "task" and ownerId == ""
      action: allow
    - capability: salesforce.com/activities.log
      filter: accountId != ""
      action: allow
    # Its own deals go to the sales ops queue, never to another rep.
    - capability: salesforce.com/records.changeOwner
      filter: fromOwnerId == "005000000000001AAA" and toOwnerId == "00G000000000001EAA"
      action: allow
    - capability: salesforce.com/email.send
      filter: domain == "example.com"
      action: allow
    - capability: salesforce.com/email.send
      action: ask-human
```

## Write semantics

- Updates use `PATCH`: only the fields given change, and `null` clears one.
- `upsertRecord` creates a record or updates the one matching its external id,
  replacing the values given; it cannot tell which before Salesforce answers.
- `createRecords` and `updateRecords` use the composite sObject API with
  `allOrNone`: a failure in one record writes none, and the other records
  report `ALL_OR_NONE_OPERATION_ROLLED_BACK`.
- `createNote` creates the note and then links it to the record; if the link
  fails, it deletes the note and throws the link's error.
- `convertLead` converts in one SOAP call. Without `convertedStatus` it uses
  the org's first converted lead status.
- `runReport` with `filters` reads the report's definition and runs it with the
  filters added (AND, extending the report's own filter logic); the saved report
  does not change.
- Nothing is retried. `SalesforceError.retryable` marks `REQUEST_LIMIT_EXCEEDED`,
  `UNABLE_TO_LOCK_ROW` and HTTP 429. A 5xx or a transport failure on a create
  may come after Salesforce applied it: look for the record before creating it
  again, or give the object an external id and use `upsertRecord`.
- A failure of the HTTP transport itself throws a plain `Error` from
  `submilli:http`.

## Queries

`query`, `aggregate`, `search` and `exportQuery` take typed descriptions and
write the SOQL or SOSL themselves: names must be API names, and every value is
encoded as a literal, so no program text reaches a query unescaped. Typed finds
and gets select `FIELDS(STANDARD)`, the standard fields the user may see, plus
the relationship names they return (`Owner.Name`, `Account.Name`), so a field
hidden by field-level security reads as null rather than failing the query.
Pages always send a size (`Sforce-Query-Options: batchSize`). Nothing loops
inside the package except `exportQuery`, which polls its Bulk API job a
bounded number of times and returns `done: false` with the job's id when it is
not finished.

## Development

```sh
cargo run -p submilli -- build test -p @submilli/salesforce --skip-network
cargo test -p submilli --test package_policy
```

`tests/lib.test.ts` runs offline: id normalization, SOQL
and SOSL escaping, the query, aggregate and search builders, response mapping,
the `convertLead` envelope and its parser, report flattening (tabular, summary
and matrix), feed paging and email addresses, and input validation before any
check or request.

The policy scripts in `tests/policy/` run as a real `main` caller under
restricted blueprints, without a token or network:

- `rep-scope.ts` shows owner-scoped finds, a query rule that refuses `Amount`
  read, filtered or sorted on, search without users, tasks created without an
  owner, cases without assignment, raw SOQL denied, and `OwnerId` refused in an
  update.
- `assistant-scope.ts` shows email only within one domain, one flow, one
  report (by a 15-character id too), its own approvals queue, calls logged, and
  metadata read but not the org's limits.

`salesforce_owner_rules_hold_against_a_fake_salesforce` and
`salesforce_process_rules_hold_against_a_fake_salesforce` in
`crates/submilli/tests/package_policy.rs` cover what needs Salesforce's
answers. Each publishes a copy of the package whose org URL points at a fake
Salesforce on `127.0.0.1` and runs a script from `tests/fake-host/`:

- `owner-rules.ts`: the looked-up owner reaches each check, another owner's
  record and a missing one are denied alike, a stage change is allowed where an
  amount change is not.
- `process-rules.ts`: a deal handed to a queue but not to a person, a lead
  converted through the SOAP API only by its owner and without an opportunity,
  a call logged on a deal's account but not on a missing deal, an approval
  approved but not rejected, and a post without mentions.

Both assert that only the allowed writes reach the fake.

Live tests run only with a token, never under `--skip-network`, against the
org the package is built for:

```sh
cargo run -p submilli -- build test -p @submilli/salesforce \
  --config orgUrl=https://<org>.my.salesforce.com --env-file <file>
```

- `tests/network_read.test.ts` needs `SALESFORCE_ACCESS_TOKEN` and only reads.
  It skips the steps that need data the org lacks (an account, a report it may
  run, a dashboard).
- `tests/network_mutation.test.ts` also needs `SALESFORCE_LIVE_MUTATIONS=true`.
  It creates accounts, a contact, a case, a task, an event, a note, a file, a
  Chatter post and a lead named with a run id, converts the lead, and deletes
  them all at the end, also when a step fails. It sends no email, submits no
  approval and runs no flow. The org needs Notes turned on (Setup, Notes
  Settings).

A client-credentials token works: an External Client App with the `api` scope
and the client credentials flow, run as an admin user.
