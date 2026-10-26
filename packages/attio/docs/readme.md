# Attio package guide

Use `@submilli/attio` to read and write records in an Attio CRM workspace:
people, companies, deals and custom objects, their stages, and notes.
Credentials are supplied internally; never pass a key through function
arguments. Every call is synchronous and returns its result.

- Objects and attributes are named by their Attio slug, such as `deals`,
  `companies`, `people`, `name`, `stage` or `associated_company`. Slugs are
  trimmed and lowercased; an object or attribute id (UUID) is refused.
- Record ids are UUIDs, such as `record.recordId`.
- Nothing can be deleted, and objects, attributes and statuses cannot be
  created or changed.

<!-- submilli:if any="attio.com/records.get attio.com/records.query attio.com/people.find attio.com/people.upsert attio.com/companies.upsert attio.com/records.create attio.com/records.update attio.com/records.setStatus" -->

## Records

A record comes back as `AttioRecord`: `object`, `recordId`, `webUrl` (a link
to open it in Attio), `createdAt`, and `values`, the current values keyed by
attribute slug. Each attribute holds a list of `RecordValue`, empty attributes
are left out, and each value has a readable `value` string:

| Attribute type | `value` | Also set |
| --- | --- | --- |
| status (such as `stage`), select | the title | `optionId` |
| record-reference (such as `associated_company`) | the referenced record id | `targetObject`, `targetRecordId` |
| actor-reference (such as `owner`) | the workspace member id (match it with `memberId` from `listMembers`) | `actorType`, `actorId` |
| currency | amount and code, such as `5000 USD` | `number`, `currencyCode` |
| number, rating | the number as text | `number` |
| text, date, timestamp, email, domain, personal name, phone | the text, date, address, domain, full name or number | |

`record.values["stage"]` is undefined when the attribute is empty; otherwise
its first element is the current value.

<!-- submilli:endif -->

<!-- submilli:if any="attio.com/records.create attio.com/records.update" -->

## Writing values

Writes take a list of `AttributeValue` entries. Each entry names its
`attribute`, a `type`, and the one field that type uses:

| `type` | Field | Writes to |
| --- | --- | --- |
| `text` | `text` | text attributes, such as a deal's `name` |
| `number` | `number` | number and rating attributes |
| `currency` | `amount` | currency attributes, such as a deal's `value`, in the attribute's own currency |
| `checkbox` | `checked` | checkbox attributes |
| `date` | `date` as `YYYY-MM-DD` | date and timestamp attributes |
| `select` | `option`, a title or option id | select attributes |
| `email` | `email` | email attributes |
| `domain` | `domain` | domain attributes |
| `record` | `targetObject` and `targetRecordId` | record references, such as `associated_company` |
| `member` | `email` of a workspace member | actor references, such as a deal's `owner` |
| `personal-name` | `firstName`, `lastName` and/or `fullName`; replaces all three parts, so give every part you want to keep (a part left out becomes "", and the full name defaults to first and last joined) | a person's `name` |

Repeat an attribute to give a multi-value attribute, such as
`associated_people`, several values. A single-value attribute takes one.
Status attributes, such as a deal's `stage`, are never written this way.
Dates must be real calendar dates, and a blank name part is refused.

<!-- submilli:endif -->

<!-- submilli:if any="attio.com/objects.list attio.com/attributes.list attio.com/statuses.list attio.com/members.list" -->

## Discover the workspace

<!-- submilli:endif -->

<!-- submilli:if capability="attio.com/objects.list" -->

- `listObjects()` lists the objects and their slugs. The `deals` object
  exists only when it is enabled in the workspace.

<!-- submilli:endif -->

<!-- submilli:if capability="attio.com/attributes.list" -->

- `listAttributes(object)` lists an object's active attributes, each with
  `attributeId`, `slug`, `title`, `description`, `type`, `isMultiselect`,
  `isWritable`, `isRequired`, `isUnique` and `currencyCode` (for a currency
  attribute, otherwise ""). Use it to learn the slugs to write and filter on.

<!-- submilli:endif -->

<!-- submilli:if capability="attio.com/statuses.list" -->

- `listStatuses(object, attribute)` lists the live statuses of a status
  attribute, such as the stages of `deals.stage`, each with `statusId`,
  `title` and `celebrationEnabled` (true for a status Attio celebrates,
  typically a won stage). Stage titles differ between workspaces, so list
  them rather than guessing.

<!-- submilli:endif -->

<!-- submilli:if capability="attio.com/members.list" -->

- `listMembers()` lists the workspace members with `memberId` and a
  lowercased `email`.

<!-- submilli:endif -->

<!-- submilli:if capability="attio.com/records.get" -->

## Get a record

`getRecord(object, recordId)` returns the record, or `null` when Attio answers
not found: no such record, or no such object.

<!-- submilli:endif -->

<!-- submilli:if capability="attio.com/records.query" -->

## Query records

`queryRecords(object, query?)` returns one page of records: `items`,
`nextOffset` and `isComplete`. To read the next page, call it again with
`offset: page.nextOffset` while `isComplete` is false, and stop when it is
true; a call past the end returns an empty page. It never fetches more than
one page.

- `filter` is a list of conditions that must all match. A condition is
  `{ attribute, operator, value }`, with an optional `field` for one property
  of a value, such as `email_domain` of an email address, `domain` or
  `root_domain` of a domain, or `first_name` of a person's name.
- `eq` compares exactly, and values are sent as given. A status or select
  attribute compares the title exactly as given, case included, such as
  `{ attribute: "stage", operator: "eq", value: "Lead" }`; take titles from
  `listStatuses`. (`setStatus` and `createRecord` match a status title without
  regard to case, but a query does not.) A record reference takes
  `{ targetObject, targetRecordId }`, and a workspace-member attribute such as
  `owner` takes `{ memberId }`.
- `contains`, `starts_with` and `ends_with` take a string; `contains`
  ignores case. For an email address or a domain, Attio needs the property
  in `field`: `{ attribute: "domains", field: "domain", operator: "contains", value: "acme" }`.
  `lt`, `lte`, `gt` and `gte` take a number or a date string.
- A domain or email in a filter is not normalized. Pass it through
  `normalizeDomain` or `normalizeEmail` first, which give the spelling Attio
  stores (`https://www.Acme.com/x` becomes `acme.com`).
- `sorts` is a list of `{ attribute, direction }`, with `direction` `asc` or
  `desc` and an optional `field`.
- `limit` is 1 to 500 and defaults to 50; `offset` defaults to 0.

```ts
import { queryRecords } from "@submilli/attio";

function main(): string[] {
    const page = queryRecords("deals", {
        filter: [{ attribute: "name", operator: "contains", value: "acme" }],
        sorts: [{ attribute: "created_at", direction: "desc" }],
        limit: 10,
    });
    const lines: string[] = [];
    for (const deal of page.items) {
        const name = deal.values["name"];
        const stage = deal.values["stage"];
        lines.push((name === undefined ? "(unnamed)" : name[0].value) + ": " + (stage === undefined ? "no stage" : stage[0].value));
    }
    return lines;
}
```

Reading further pages, at most three here:

```ts
import { normalizeDomain, QueryCondition, queryRecords } from "@submilli/attio";

function main(): number {
    const byDomain: QueryCondition = { attribute: "domains", operator: "eq", value: normalizeDomain("https://www.Acme.com/") };
    let page = queryRecords("companies", { filter: [byDomain], limit: 2 });
    let count = page.items.length;
    for (let pages = 1; pages < 3 && !page.isComplete; pages += 1) {
        page = queryRecords("companies", { filter: [byDomain], limit: 2, offset: page.nextOffset });
        count += page.items.length;
    }
    return count;
}
```

<!-- submilli:endif -->

<!-- submilli:if any="attio.com/people.find attio.com/people.upsert attio.com/companies.upsert" -->

## People and companies

These operations normalize what they are given: an email is trimmed and
lowercased, and a domain becomes a bare lowercase domain, so
`https://www.Acme.com/pricing` becomes `acme.com`. Give an internationalized
domain in punycode (`xn--…`). Queries do not normalize; use `normalizeDomain`
and `normalizeEmail` for them.

<!-- submilli:endif -->

<!-- submilli:if capability="attio.com/people.find" -->

- `findPersonByEmail(email)` returns the person with that address, or `null`.

<!-- submilli:endif -->

<!-- submilli:if capability="attio.com/people.upsert" -->

- `upsertPerson({ email, firstName?, lastName?, fullName?, jobTitle?, description?, companyRecordId? })`
  creates the person, or updates the person who has that email. On an
  existing person each field given replaces its current value, and a name
  replaces the whole current name; fields left out, and the person's other
  email addresses, stay. Give only the fields you mean to set.

<!-- submilli:endif -->

<!-- submilli:if capability="attio.com/companies.upsert" -->

- `upsertCompany({ domain, name?, description? })` creates the company, or
  updates the company that has that domain. On an existing company each
  field given replaces its current value; fields left out, and the company's
  other domains, stay.

<!-- submilli:endif -->

<!-- submilli:if any="attio.com/people.upsert attio.com/companies.upsert" -->

When more than one record already has the email or domain, the upsert fails
with code `multiple_match_results` and changes nothing.

<!-- submilli:endif -->

<!-- submilli:if capability="attio.com/records.create" -->

## Create a record

`createRecord(object, { values?, status? })` creates a record and returns it.
`status` sets one status attribute at creation, such as
`{ attribute: "stage", title: "Lead" }`; the title is matched without regard
to case, and a status id also works. Giving a status also needs permission to
list that attribute's statuses. Attributes with `isRequired` must be
given: a deal needs its `name`, an `owner` (a workspace member's email) and a
`stage`. Without one, Attio refuses the create with an `AttioError` of status
400 and Attio's own code (its reference does not name it for creates).
Each call reads the object's attribute definitions, and with a status the
attribute's statuses, before it writes: one or two extra reads per create.

`createRecord` never matches an existing record. A person's email and a
company's domain are unique, so creating a person or company that already
exists fails with code `uniqueness_conflict`: look it up first with
`findPersonByEmail` or `queryRecords`, and create it only when it is missing.
`createRecord` normalizes a domain value, but a query compares the value as
given, so pass the query `normalizeDomain(domain)` (or a bare lowercase domain
such as `acme.com`).

A record reference also writes the other side of its relationship: a new
person's `company` adds the person to that company's `team`. Where the other
side holds a single value, as a person's `company` does, the write replaces
it, so a new company whose `team` lists an existing person moves that person
to the new company.

<!-- submilli:endif -->

<!-- submilli:if all="attio.com/records.create attio.com/people.find" -->

```ts
import { createRecord, findPersonByEmail } from "@submilli/attio";

function main(): string {
    const existing = findPersonByEmail("dana@acme.com");
    if (existing !== null) return existing.recordId;
    const person = createRecord("people", { values: [
        { attribute: "email_addresses", type: "email", email: "dana@acme.com" },
        { attribute: "name", type: "personal-name", firstName: "Dana", lastName: "Lee" },
    ] });
    return person.recordId;
}
```

<!-- submilli:endif -->

<!-- submilli:if capability="attio.com/records.update" -->

## Update a record

`updateRecord(object, recordId, values)` changes only the attributes given
and returns the record. A multi-value attribute gains the values given and
keeps its others, so adding a person to `associated_people` keeps the people
already linked. A single-value attribute is replaced; a `personal-name` value
replaces all three parts of the name, so give every part you want to keep. A
status attribute is refused with code `status_attribute`.

<!-- submilli:endif -->

<!-- submilli:if all="attio.com/records.setStatus attio.com/statuses.list attio.com/records.get" -->

## Change a status

`setStatus(object, recordId, attribute, status)` moves a record to another
live status, such as a deal to another stage, Won and Lost included. `status`
is a title, matched without regard to case, or a status id. An unknown status
fails with code `unknown_status`, and the message lists the valid titles. The
result is a `StatusChange`: the `record`, the `status` title now set, the
`previousStatus` title (`""` when there was none), and `changed`, false when
the record already had that status and nothing was written. It first lists
the attribute's statuses and reads the record, so it needs permission for
both reads. On an object without that status attribute, or for a missing
record, it fails with an `AttioError` of status 404.

<!-- submilli:endif -->

<!-- submilli:if any="attio.com/notes.create attio.com/notes.list" -->

## Notes

<!-- submilli:endif -->

<!-- submilli:if capability="attio.com/notes.create" -->

- `createNote(parentObject, parentRecordId, { title, content, format? })`
  attaches a note to a record. `format` is `plaintext` (the default) or
  `markdown`.

<!-- submilli:endif -->

<!-- submilli:if capability="attio.com/notes.list" -->

- `listNotes(parentObject, parentRecordId, page?)` returns one page of that
  record's notes, with `limit` 1 to 50 (default 50) and `offset`; continue
  with `nextOffset` while `isComplete` is false. Each note has `title`,
  `text` (plain text) and `markdown`.

<!-- submilli:endif -->

## Errors

- `getRecord` returns `null` when Attio answers not found, which it does both
  for a missing record and for an unknown object slug; `findPersonByEmail`
  returns `null` when nobody has the email.
- Other failures from Attio throw `AttioError` with `code`, `message` and
  `status`. `status` 0 is an error the package raised itself: invalid input
  such as an email, slug, record id, value or page size (refused before
  anything is sent), an unknown status, a status attribute in a plain write,
  or two people found with one email. Fix the argument rather than retrying.
  Otherwise `status` is Attio's HTTP status and `code` Attio's own, such as
  `value_not_found` for an unknown option or member.
- `retryable` is true when Attio did not apply the request: a rate limit
  (status 429) or a concurrent write (status 409, code
  `concurrent_write_conflict`). `retryAfterSeconds` is how long Attio asked
  to wait, in seconds, or null when it did not say, as for a 409. A program
  cannot sleep, so do not retry at once: stop and report. Nothing is retried
  automatically.
- A failed connection or timeout throws a plain `Error` from the HTTP
  transport, not `AttioError`. Catch `AttioError` first, then `Error`.
- A denied operation throws `PermissionDeniedError`, a runtime global rather
  than an export of this package. The operation is not allowed with those
  arguments, so do not retry it with other spellings.

```ts
import { AttioError, getRecord } from "@submilli/attio";

function main(): string {
    try {
        const deal = getRecord("deals", "8a1b2c3d-1111-4222-8333-944455556666");
        return deal === null ? "no such deal" : deal.webUrl;
    } catch (error: PermissionDeniedError) {
        return "not allowed: " + error.capability;
    } catch (error: AttioError) {
        if (!error.retryable) return "Attio refused: " + error.code;
        const wait = error.retryAfterSeconds;
        return wait === null ? "Attio is busy (" + error.code + "); retry later" : "Attio is busy; retry in " + wait.toString() + " s";
    } catch (error) {
        return "could not reach Attio: " + error.message;
    }
}
```

### Writes with an unknown outcome

A write that fails with a 5xx status or a transport error may still have been
applied: there is no idempotency key, and `retryable` is false. Before
retrying, look for what the write would have made: a deal with
`queryRecords("deals", { filter: [{ attribute: "name", operator: "eq", value: name }] })`,
a company by its normalized domain, a person with `findPersonByEmail`, a
note in `listNotes` of its record. Retry only when it is missing. The robust
fix is a unique attribute on the object (such as the id of the message a deal
came from): a repeated create then fails with `uniqueness_conflict` instead of
making a duplicate.

<!-- submilli:if all="attio.com/records.query attio.com/people.find attio.com/records.create attio.com/statuses.list attio.com/notes.create" -->

## Example

Record a new lead: find the company by its normalized domain and the contact
by email, create each only when it is missing, then create a deal in the Lead
stage, owned by a workspace member and linked to both, with a note holding
the source text.

```ts
import { AttioRecord, createNote, createRecord, findPersonByEmail, normalizeDomain, queryRecords } from "@submilli/attio";

function companyFor(url: string, name: string): AttioRecord {
    // A query compares the value as given, so normalize it as createRecord does.
    const domain = normalizeDomain(url);
    const found = queryRecords("companies", { filter: [{ attribute: "domains", operator: "eq", value: domain }], limit: 1 });
    if (found.items.length > 0) return found.items[0];
    return createRecord("companies", { values: [
        { attribute: "domains", type: "domain", domain: domain },
        { attribute: "name", type: "text", text: name },
    ] });
}

function main(): { deal: string; url: string } {
    const company = companyFor("https://www.acme.com/", "Acme");
    const person = findPersonByEmail("dana@acme.com") ?? createRecord("people", { values: [
        { attribute: "email_addresses", type: "email", email: "dana@acme.com" },
        { attribute: "name", type: "personal-name", firstName: "Dana" },
        { attribute: "company", type: "record", targetObject: "companies", targetRecordId: company.recordId },
    ] });
    const deal = createRecord("deals", {
        values: [
            { attribute: "name", type: "text", text: "Acme demo" },
            { attribute: "owner", type: "member", email: "sam@pied-piper.example" },
            { attribute: "associated_company", type: "record", targetObject: "companies", targetRecordId: company.recordId },
            { attribute: "associated_people", type: "record", targetObject: "people", targetRecordId: person.recordId },
        ],
        status: { attribute: "stage", title: "Lead" },
    });
    createNote("deals", deal.recordId, { title: "Lead from the meetup", content: "Met Dana from Acme; wants a demo next week." });
    return { deal: deal.recordId, url: deal.webUrl };
}
```

<!-- submilli:endif -->
