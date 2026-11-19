# Salesforce package guide

Use `@submilli/salesforce` to read and change a Salesforce org: accounts,
contacts, leads, opportunities, cases and the other standard objects, any
custom object, and queries, totals and search over them; activities, notes,
files, Chatter and email; approvals, flows, lead conversion and owners; and
reports, dashboards, list views and the org's metadata. Credentials and the
org's address are supplied internally; never pass a token or URL. Every call
is synchronous and returns its result.

- Objects and fields are named by their API name, such as `Opportunity`,
  `StageName`, `Invoice__c` or `Region__c`, in any case. A parent's field is a
  path, such as `Account.Name` or `Owner.Email`.
- Record ids are Salesforce ids, 15 or 18 characters, such as `record.id`.
  Results always carry the 18-character form.
- Salesforce applies its own sharing and field-level security: you see and
  change only what the org lets this user. A field you may not see reads as
  null in typed results, and fails a `query` that names it.
- A denial (`PermissionDeniedError`) means the operation is forbidden with
  these arguments, not that it was called wrongly: do not retry it with other
  spellings. Reading a record that does not exist is denied the same way as one
  you may not read.

<!-- submilli:if any="salesforce.com/accounts.find salesforce.com/contacts.find salesforce.com/leads.find salesforce.com/opportunities.find salesforce.com/cases.find salesforce.com/opportunityLineItems.find salesforce.com/products.find salesforce.com/pricebooks.find salesforce.com/tasks.find salesforce.com/events.find salesforce.com/users.find salesforce.com/campaigns.find salesforce.com/campaignMembers.find salesforce.com/contracts.find salesforce.com/orders.find salesforce.com/query.run salesforce.com/query.soql salesforce.com/records.related" -->

## Pages

Finds and queries return a `Page`: `records`, `totalSize` (everything that
matches), `done`, and `nextCursor`. While `done` is false, call the same
function with the same arguments and `{ cursor: page.nextCursor }` for the next
page. `pageSize` is 200 to 2000 (default 200). Read only the pages you need:
`totalSize` answers "how many" without reading them, and `aggregate` totals
without reading records at all.

<!-- submilli:endif -->

<!-- submilli:if any="salesforce.com/accounts.find salesforce.com/contacts.find salesforce.com/leads.find salesforce.com/opportunities.find salesforce.com/cases.find salesforce.com/opportunityLineItems.find salesforce.com/products.find salesforce.com/pricebooks.find salesforce.com/tasks.find salesforce.com/events.find salesforce.com/users.find salesforce.com/campaigns.find salesforce.com/campaignMembers.find salesforce.com/contracts.find salesforce.com/orders.find salesforce.com/accounts.get salesforce.com/contacts.get salesforce.com/leads.get salesforce.com/opportunities.get salesforce.com/cases.get salesforce.com/opportunityLineItems.get salesforce.com/products.get salesforce.com/pricebooks.get salesforce.com/tasks.get salesforce.com/events.get salesforce.com/users.get salesforce.com/campaigns.get salesforce.com/campaignMembers.get salesforce.com/contracts.get salesforce.com/orders.get" -->

## Standard objects

Each standard object has a typed result (`Account`, `Contact`, `Lead`,
`Opportunity`, `OpportunityLineItem`, `Product`, `Pricebook`, `Case`, `Task`,
`Event`, `User`, `Campaign`, `CampaignMember`, `Contract`, `Order`) with
camel-case fields, such as `opportunity.stage`, `opportunity.closeDate` and
`opportunity.accountName`. Each field's documentation names its API field.
Fields the org leaves empty are null; checkboxes are `true` or `false`.

`find…(filters, options)` returns a page, most recently changed first; every
filter given must match, and a filter left out does not filter. `get…(id)`
returns one record or null. To read custom or other fields, list them in
`options.fields`; they come back in `custom`, keyed as you wrote them.

<!-- submilli:endif -->

<!-- submilli:if capability="salesforce.com/accounts.find" -->
- `findAccounts({ name, ownerId, industry, type, modifiedAfter })`: `name` matches text the name contains.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/accounts.get" -->
- `getAccount(id)`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/contacts.find" -->
- `findContacts({ accountId, email, name })`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/contacts.get" -->
- `getContact(id)`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/leads.find" -->
- `findLeads({ ownerId, status, email, company, isConverted })`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/leads.get" -->
- `getLead(id)`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/opportunities.find" -->
- `findOpportunities({ accountId, ownerId, stage, isClosed, isWon, closeAfter, closeBefore, minAmount })`: dates are `YYYY-MM-DD`, inclusive.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/opportunities.get" -->
- `getOpportunity(id)`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/opportunityLineItems.find" -->
- `findOpportunityLineItems({ opportunityId })`: an opportunity's products.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/opportunityLineItems.get" -->
- `getOpportunityLineItem(id)`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/products.find" -->
- `findProducts({ name, family, isActive })`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/products.get" -->
- `getProduct(id)`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/pricebooks.find" -->
- `findPricebooks({ isActive, isStandard })`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/pricebooks.get" -->
- `getPricebook(id)`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/cases.find" -->
- `findCases({ accountId, ownerId, contactId, status, priority, isClosed, createdAfter })`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/cases.get" -->
- `getCase(id)`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/tasks.find" -->
- `findTasks({ whatId, whoId, ownerId, status, isClosed, dueAfter, dueBefore })`: `whatId` is the related record (an account, opportunity or case), `whoId` the person.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/tasks.get" -->
- `getTask(id)`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/events.find" -->
- `findEvents({ whatId, whoId, ownerId, startAfter, startBefore })`: date-times with a zone.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/events.get" -->
- `getEvent(id)`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/users.find" -->
- `findUsers({ name, email, isActive })`: the org's users, such as owners to look up by name.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/users.get" -->
- `getUser(id)`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/campaigns.find" -->
- `findCampaigns({ name, ownerId, status, type, isActive })`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/campaigns.get" -->
- `getCampaign(id)`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/campaignMembers.find" -->
- `findCampaignMembers({ campaignId, leadId, contactId })`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/campaignMembers.get" -->
- `getCampaignMember(id)`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/contracts.find" -->
- `findContracts({ accountId, ownerId, status })`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/contracts.get" -->
- `getContract(id)`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/orders.find" -->
- `findOrders({ accountId, ownerId, status })`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/orders.get" -->
- `getOrder(id)`.
<!-- submilli:endif -->

<!-- submilli:if capability="salesforce.com/opportunities.find" -->

Open deals closing this year, with a custom field:

```ts
import { findOpportunities } from "@submilli/salesforce";

function main(): { name: string | null; region: string | null }[] {
    const page = findOpportunities({ isClosed: false, closeBefore: "2026-12-31" }, { fields: ["Region__c"] });
    const rows: { name: string | null; region: string | null }[] = [];
    for (const deal of page.records) {
        const region = deal.custom["Region__c"];
        rows.push({ name: deal.name, region: typeof region === "string" ? region : null });
    }
    return rows;
}
```

<!-- submilli:endif -->

<!-- submilli:if any="salesforce.com/contacts.create salesforce.com/leads.create salesforce.com/leads.update salesforce.com/opportunities.update salesforce.com/cases.create salesforce.com/cases.update" -->

## Typed writes

Creates return the new record's id. Updates change only the fields given;
`null` clears a field. Other fields, such as custom ones, go in `custom` as
`[{ field: "Region__c", value: "EMEA" }]`. Owners cannot be changed by an
update.

<!-- submilli:endif -->

<!-- submilli:if capability="salesforce.com/contacts.create" -->
- `createContact({ lastName, firstName, accountId, email, phone, title, … })`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/leads.create" -->
- `createLead({ lastName, company, firstName, email, status, … })`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/leads.update" -->
- `updateLead(id, { status, email, rating, … })`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/opportunities.update" -->
- `updateOpportunity(id, { stage, closeDate, amount, probability, nextStep, … })`: `stage` is the stage's name, such as `Closed Won`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/cases.create" -->
- `createCase({ subject, description, status, priority, origin, accountId, contactId, … })`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/cases.update" -->
- `updateCase(id, { status, priority, description, … })`.
<!-- submilli:endif -->

<!-- submilli:if any="salesforce.com/records.get salesforce.com/records.create salesforce.com/records.update salesforce.com/records.delete salesforce.com/records.upsert salesforce.com/records.createMany salesforce.com/records.updateMany salesforce.com/records.related salesforce.com/records.updated salesforce.com/records.deleted salesforce.com/query.run salesforce.com/query.search salesforce.com/query.soql salesforce.com/query.sosl" -->

## Records of any object

Operations on any object, custom ones included, return a `SalesforceRecord`:
`object`, `id`, and `fields`, the values read keyed by API name as Salesforce
spells it. A parent's fields are keyed by path, such as `fields["Account.Name"]`,
and so are a compound field's parts, such as `fields["BillingAddress.city"]`.
Writes take a list of `{ field, value }`: text (also dates `YYYY-MM-DD`,
date-times and picklist values), numbers, checkboxes, or null to clear.

<!-- submilli:endif -->

<!-- submilli:if capability="salesforce.com/records.get" -->
- `getRecord(object, id, { fields })`: every readable field, or the ones listed; null when it does not exist.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/records.create" -->
- `createRecord(object, values)`: returns the id. `OwnerId` among the values sets the owner.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/records.update" -->
- `updateRecord(object, id, values)`: `OwnerId` is refused.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/records.delete" -->
- `deleteRecord(object, id)`: moves it to the recycle bin; false when it did not exist.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/records.upsert" -->
- `upsertRecord(object, externalIdField, externalId, values)`: creates the record, or updates the one with that external id, replacing the values given. Returns `{ id, created }`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/records.createMany" -->
- `createRecords(object, records)`: up to 200 in one request, all or none. Returns each record's `{ id, success, errors }`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/records.updateMany" -->
- `updateRecords(object, [{ id, values }])`: up to 200, all or none.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/records.related" -->
- `getRelated(object, id, relationship, { fields })`: a record's related records through a relationship name, such as an account's `Contacts`, `Opportunities` or `Cases`; one page.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/records.updated" -->
- `getUpdated(object, start, end)`: ids created or changed between two date-times, within the last 30 days.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/records.deleted" -->
- `getDeleted(object, start, end)`: records deleted in the window, within the last 15 days.
<!-- submilli:endif -->

<!-- submilli:if capability="salesforce.com/query.run" -->

## Query

`query({ object, fields, where, orderBy, limit }, page)` reads one object's
records. The package writes the SOQL and encodes every value, so pass values as
they are, without quotes or escaping. Conditions in `where` are combined with
AND; each gives `field`, `op` and exactly one of:

| Give | For | Example |
| --- | --- | --- |
| `value` | text, a number, a checkbox, or null (`=`, `!=`) | `{ field: "StageName", op: "=", value: "Closed Won" }` |
| `values` | `in`, `not in`; `includes`, `excludes` on multi-select picklists | `{ field: "StageName", op: "in", values: ["Prospecting", "Qualification"] }` |
| `date` | a date field, `YYYY-MM-DD` | `{ field: "CloseDate", op: "<=", date: "2026-12-31" }` |
| `dateTime` | a date-time field, with a zone | `{ field: "CreatedDate", op: ">", dateTime: "2026-11-01T00:00:00Z" }` |
| `dateLiteral` | a relative date | `{ field: "CloseDate", op: "=", dateLiteral: "THIS_QUARTER" }` or `"LAST_N_DAYS:30"` |

`like` matches a pattern where `%` is any text and `_` one character. `limit`
caps the records across all pages. A query that names a field the user may not
see fails with `INVALID_FIELD`.

<!-- submilli:endif -->

<!-- submilli:if capability="salesforce.com/query.aggregate" -->

## Totals

`aggregate({ object, groupBy, measures, where, orderBy, limit })` counts, sums,
averages and finds minimums and maximums without reading records. Each measure
is `{ fn, field }` with `fn` one of `count` (no field needed), `count_distinct`,
`sum`, `avg`, `min`, `max`. Group by fields, or by part of a date with `date`:
`calendar_month`, `calendar_quarter`, `calendar_year`, `fiscal_quarter`,
`fiscal_year`, `day_only` or `week_in_year`. Rows come back keyed by label: a
group's field as written, or `calendar_month(CloseDate)`; a measure's
`count()` or `sum(Amount)`. Sort by those labels. At most 2000 rows come back.

```ts
import { aggregate } from "@submilli/salesforce";

function main(): { stage: string; deals: number; amount: number }[] {
    const result = aggregate({
        object: "Opportunity",
        groupBy: [{ field: "StageName" }],
        measures: [{ fn: "count" }, { fn: "sum", field: "Amount" }],
        where: [{ field: "IsClosed", op: "=", value: false }],
        orderBy: [{ by: "sum(Amount)", direction: "desc" }],
    });
    const rows: { stage: string; deals: number; amount: number }[] = [];
    for (const row of result.rows) {
        const stage = row.groups["StageName"];
        const deals = row.measures["count()"];
        const amount = row.measures["sum(Amount)"];
        rows.push({
            stage: typeof stage === "string" ? stage : "",
            deals: typeof deals === "number" ? deals : 0,
            amount: typeof amount === "number" ? amount : 0,
        });
    }
    return rows;
}
```

<!-- submilli:endif -->

<!-- submilli:if capability="salesforce.com/query.search" -->

## Search

`search({ text, objects: [{ object, fields, limit }], limit })` finds text,
such as a name, email or phone number, across objects, best matches first. The
text is searched as written. List the fields to return for each object; `Id`
always comes back.

<!-- submilli:endif -->

<!-- submilli:if capability="salesforce.com/query.export" -->

## Export

`exportQuery(spec, path)` writes every record a query matches to a CSV file
through the Bulk API, for more records than pages can carry. Salesforce runs
the job in the background: while the result's `done` is false, call again with
the same query and path and `{ jobId: result.jobId }`. The Bulk API does not
sort, so `orderBy` is refused. The file's directory must exist.

<!-- submilli:endif -->

<!-- submilli:if capability="salesforce.com/query.soql" -->
- `soql(text, page)` runs a SOQL query as written. Quote and escape values yourself.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/query.sosl" -->
- `sosl(text)` runs a SOSL search as written.
<!-- submilli:endif -->

<!-- submilli:if any="salesforce.com/activities.log salesforce.com/activities.createEvent salesforce.com/activities.list salesforce.com/notes.create salesforce.com/files.upload salesforce.com/files.download salesforce.com/feed.post salesforce.com/feed.read salesforce.com/email.send" -->

## Activities, notes, files and email

These act on one record, named by its id: an account, contact, lead,
opportunity, case or other. Activities on an account include those logged on
its contacts and opportunities.

<!-- submilli:endif -->

<!-- submilli:if capability="salesforce.com/activities.log" -->
- `logActivity({ type, subject, whatId, whoId, description, date })` logs a completed call, email or other task (`type`: `call`, `email`, `task`; it sets `TaskSubtype`). Log a meeting with `createEvent`, which takes past times too. `whatId` is the record it is about, `whoId` the contact or lead; `date` is `YYYY-MM-DD`. Returns the task's id.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/activities.createEvent" -->
- `createEvent({ subject, start, end, whatId, whoId, location, description })` puts a meeting on the calendar; `start` and `end` are date-times with a zone, such as `2026-11-20T15:00:00Z`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/activities.list" -->
- `listActivities(recordId, { open, limit })` returns the activity history, newest first, or with `open: true` the upcoming tasks and events; at most `limit` (default 50, at most 200).
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/notes.create" -->
- `createNote(recordId, title, body)` adds a note; each line of `body` is a paragraph.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/files.upload" -->
- `uploadFile(recordId, path, { title })` attaches a VFS file of at most 37 MB and returns its `contentDocumentId`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/files.download" -->
- `downloadFile(contentDocumentId, path, { overwrite })` writes a file's latest version to the VFS; the directory must exist. A record's files are its `ContentDocumentLinks` (`getRelated`).
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/feed.post" -->
- `postToFeed(recordId, text, { mentions })` posts to the record's Chatter feed; `mentions` are user ids, and each is notified.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/feed.read" -->
- `readFeed(recordId, { pageSize, cursor })` returns `{ posts, nextCursor }`, newest first, 1 to 100 per page (default 25).
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/email.send" -->
- `sendEmail({ to, subject, body, relatedTo })` sends a plain-text email as this user to 1 to 5 bare addresses (`dana@example.com`), logged on `relatedTo` when given.
<!-- submilli:endif -->

<!-- submilli:if any="salesforce.com/approvals.submit salesforce.com/approvals.approve salesforce.com/approvals.reject salesforce.com/approvals.listPending salesforce.com/flows.run salesforce.com/leads.convert salesforce.com/records.changeOwner" -->

## Approvals, flows, leads and owners

<!-- submilli:endif -->

<!-- submilli:if capability="salesforce.com/approvals.submit" -->
- `submitForApproval(recordId, { comment, processName, nextApproverIds })` submits a record; without `processName`, Salesforce picks the first process whose criteria it meets. Returns `{ instanceId, status, recordId, workItemIds, actorIds }`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/approvals.listPending" -->
- `listPendingApprovals({ actorId })` returns a page of approvals waiting for a decision: `workItemId`, the record (`recordId`, `recordName`, `object`), `processName`, and who must decide (`actorId`).
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/approvals.approve" -->
- `approve(workItemId, { comment })` approves.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/approvals.reject" -->
- `reject(workItemId, { comment })` rejects.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/flows.run" -->
- `runFlow(name, [{ name, value }])` runs an autolaunched flow by its API name with its input variables and returns `{ outputs }`; a record or collection output is its JSON text.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/leads.convert" -->
- `convertLead(leadId, { accountId, contactId, createOpportunity, opportunityName, convertedStatus, ownerId })` turns a lead into an account, a contact and, unless `createOpportunity` is false, an opportunity; give `accountId` (and `contactId`) to merge into existing records. Returns `{ accountId, contactId, opportunityId }`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/records.changeOwner" -->
- `changeOwner(object, id, newOwnerId)` gives a record to a user (`005…`) or queue (`00G…`). It is the only way to change `OwnerId`; updates refuse it.
<!-- submilli:endif -->

<!-- submilli:if any="salesforce.com/reports.list salesforce.com/reports.describe salesforce.com/reports.run salesforce.com/dashboards.list salesforce.com/dashboards.get" -->

## Reports and dashboards

`runReport(reportId, { filters, includeDetails })` returns the report flattened:
`columns` (`name`, `label`, `dataType`), `rows` (one per record, keyed by column
name, with the group values filled in), and `totals` (each subtotal with its
`groups`, then the grand total, keyed by aggregate name such as `RowCount` or
`s!AMOUNT`; `aggregates` gives their labels). Numbers, dates and booleans are
values; other cells are the text the report shows, such as an account's name.
`truncated` is true when Salesforce stopped at 2000 rows: use `aggregate` or
`exportQuery` for the rest. `filters` (`{ column, operator, value }`, operators
such as `equals`, `notEqual`, `greaterThan`, `contains`) are added to the
report's own for this run; `describeReport` lists its columns and filters.
`includeDetails: false` returns totals only.

<!-- submilli:endif -->

<!-- submilli:if capability="salesforce.com/reports.list" -->
- `listReports({ name, folder })` returns a page of reports, by name; `name` matches text the name contains.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/reports.describe" -->
- `describeReport(reportId)` returns its format, columns, groupings, aggregates and saved filters, or null.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/dashboards.list" -->
- `listDashboards({ name, folder })` returns a page of dashboards, by title.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/dashboards.get" -->
- `getDashboard(dashboardId)` returns its components, each with its source `reportId` and the `totals` it shows as of its last refresh, or null.
<!-- submilli:endif -->

<!-- submilli:if any="salesforce.com/listViews.list salesforce.com/listViews.run salesforce.com/objects.list salesforce.com/objects.describe salesforce.com/org.limits salesforce.com/org.currentUser" -->

## List views and metadata

<!-- submilli:endif -->

<!-- submilli:if capability="salesforce.com/listViews.list" -->
- `listListViews(object)` returns an object's list views (`id`, `label`, `developerName`), such as `My Opportunities`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/listViews.run" -->
- `runListView(object, listViewId, { pageSize, cursor })` returns a page of the view's records, with its filters and sort.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/objects.list" -->
- `listObjects()` returns the org's objects: `name`, `label`, `custom`, `queryable`, `keyPrefix`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/objects.describe" -->
- `describeObject(object)` returns its fields (`name`, `type`, `picklistValues`, `referenceTo`, `relationshipName`, `createable`, `updateable`), child relationships and record types, or null. Use it before working with a custom object.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/org.limits" -->
- `getLimits()` returns the org's API limits, such as `DailyApiRequests`, with `max` and `remaining`.
<!-- submilli:endif -->
<!-- submilli:if capability="salesforce.com/org.currentUser" -->
- `getCurrentUser()` returns the user this connection acts as: `id`, `name`, `email`.
<!-- submilli:endif -->

## Errors

Salesforce's errors throw `SalesforceError` with `errorCode` (such as
`REQUIRED_FIELD_MISSING`, `INVALID_FIELD`, `MALFORMED_QUERY`,
`FIELD_CUSTOM_VALIDATION_EXCEPTION` or `INVALID_SESSION_ID`), `fields` (the
fields it names), `status` (HTTP), and `retryable`. Fix the input and try again
only when the error names something you can change; report
`INVALID_SESSION_ID` (the connection expired) rather than retrying. When
`retryable` is true (`REQUEST_LIMIT_EXCEEDED`, `UNABLE_TO_LOCK_ROW`), stop and
report: a program cannot wait. Input refused before any request throws
`SalesforceError` with `status` 0 and a lowercase code, such as `invalid_name`,
`invalid_id`, `invalid_condition`, `invalid_recipient` or `owner_field`. A
missing record returns null, or false from `deleteRecord`.

<!-- submilli:if all="salesforce.com/accounts.find salesforce.com/opportunities.find salesforce.com/query.aggregate" -->

## Example

Brief an account: find it by name, then its open pipeline and total won.

```ts
import { aggregate, findAccounts, findOpportunities } from "@submilli/salesforce";

interface Deal {
    name: string | null;
    stage: string | null;
    amount: number | null;
}

function main(): { account: string; open: Deal[]; won: number } {
    const accounts = findAccounts({ name: "Acme" });
    if (accounts.records.length === 0) return { account: "not found", open: [], won: 0 };
    const account = accounts.records[0];
    const open: Deal[] = [];
    for (const deal of findOpportunities({ accountId: account.id, isClosed: false }).records) {
        open.push({ name: deal.name, stage: deal.stage, amount: deal.amount });
    }
    const won = aggregate({
        object: "Opportunity",
        measures: [{ fn: "sum", field: "Amount" }],
        where: [{ field: "AccountId", op: "=", value: account.id }, { field: "IsWon", op: "=", value: true }],
    });
    const total = won.rows.length > 0 ? won.rows[0].measures["sum(Amount)"] : null;
    return { account: account.name ?? account.id, open: open, won: typeof total === "number" ? total : 0 };
}
```

<!-- submilli:endif -->

<!-- submilli:if all="salesforce.com/activities.log salesforce.com/opportunities.update" -->

## Example: log a call and move the deal

```ts
import { logActivity, updateOpportunity } from "@submilli/salesforce";

function main(): string {
    const dealId = "006000000000001";
    const taskId = logActivity({
        type: "call",
        subject: "Call with Dana: agreed on terms",
        whatId: dealId,
        description: "Legal review next week.",
        date: "2026-11-18",
    });
    updateOpportunity(dealId, { stage: "Negotiation/Review", nextStep: "Legal review" });
    return taskId;
}
```

<!-- submilli:endif -->

<!-- submilli:if all="salesforce.com/reports.list salesforce.com/reports.run" -->

## Example: the team's report, totals only

```ts
import { listReports, runReport, ReportTotal } from "@submilli/salesforce";

function main(): ReportTotal[] {
    const reports = listReports({ name: "Pipeline by Stage" });
    if (reports.records.length === 0) return [];
    const result = runReport(reports.records[0].id, {
        filters: [{ column: "CLOSE_DATE", operator: "greaterOrEqual", value: "2026-10-01" }],
        includeDetails: false,
    });
    return result.totals;
}
```

<!-- submilli:endif -->
