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
| actor-reference (such as `owner`) | the workspace member's email | `actorType`, `actorId` |
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

Repeat an attribute to give a multi-value attribute, such as
`associated_people`, several values. A single-value attribute takes one.
Status attributes, such as a deal's `stage`, are never written this way.

<!-- submilli:endif -->

<!-- submilli:if any="attio.com/objects.list attio.com/attributes.list attio.com/statuses.list attio.com/members.list" -->

## Discover the workspace

<!-- submilli:endif -->

<!-- submilli:if capability="attio.com/objects.list" -->

- `listObjects()` lists the objects and their slugs. The `deals` object
  exists only when it is enabled in the workspace.

<!-- submilli:endif -->

<!-- submilli:if capability="attio.com/attributes.list" -->

- `listAttributes(object)` lists an object's active attributes with `slug`,
  `type`, `isMultiselect` and `isWritable`. Use it to learn the slugs to
  write and filter on.

<!-- submilli:endif -->

<!-- submilli:if capability="attio.com/statuses.list" -->

- `listStatuses(object, attribute)` lists the live statuses of a status
  attribute, such as the stages of `deals.stage`. Stage titles differ between
  workspaces, so list them rather than guessing.

<!-- submilli:endif -->

<!-- submilli:if capability="attio.com/members.list" -->

- `listMembers()` lists the workspace members with `memberId` and a
  lowercased `email`.

<!-- submilli:endif -->

<!-- submilli:if capability="attio.com/records.get" -->

## Get a record

`getRecord(object, recordId)` returns the record, or `null` when Attio has no
such record.

<!-- submilli:endif -->

<!-- submilli:if capability="attio.com/records.query" -->

## Query records

`queryRecords(object, query?)` returns one page of records: `items`,
`nextOffset` and `isComplete`. To read the next page, call it again with
`offset: page.nextOffset` while `isComplete` is false. It never fetches more
than one page.

- `filter` is a list of conditions that must all match. A condition is
  `{ attribute, operator, value }`, with an optional `field` for one property
  of a value, such as `email_domain` of an email address, `root_domain` of a
  domain, or `first_name` of a person's name.
- `eq` compares exactly. For a status or select attribute the value is the
  title, such as `{ attribute: "stage", operator: "eq", value: "Lead" }`. For a
  record reference the value is `{ targetObject, targetRecordId }`.
- `contains`, `starts_with` and `ends_with` take a string; `contains`
  ignores case. `lt`, `lte`, `gt` and `gte` take a number or a date string.
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

<!-- submilli:endif -->

<!-- submilli:if any="attio.com/people.find attio.com/people.upsert attio.com/companies.upsert" -->

## People and companies

Email addresses are trimmed and lowercased. Domains are reduced to a bare
lowercase domain, so `https://www.Acme.com/pricing` becomes `acme.com`.

<!-- submilli:endif -->

<!-- submilli:if capability="attio.com/people.find" -->

- `findPersonByEmail(email)` returns the person with that address, or `null`.

<!-- submilli:endif -->

<!-- submilli:if capability="attio.com/people.upsert" -->

- `upsertPerson({ email, firstName?, lastName?, fullName?, jobTitle?, description?, companyRecordId? })`
  creates the person, or updates the person who has that email. Only the
  fields given are written: other values and the person's other email
  addresses stay. A name you give replaces the whole current name.

<!-- submilli:endif -->

<!-- submilli:if capability="attio.com/companies.upsert" -->

- `upsertCompany({ domain, name?, description? })` creates the company, or
  updates the company that has that domain. Only the fields given are
  written, and the company's other domains stay.

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
to case, and a status id also works. Attributes with `isRequired` must be
given: a deal needs its `name`, an `owner` (a workspace member's email) and a
`stage`, or Attio refuses it with an `AttioError`.

<!-- submilli:endif -->

<!-- submilli:if capability="attio.com/records.update" -->

## Update a record

`updateRecord(object, recordId, values)` changes only the attributes given
and returns the record. A multi-value attribute gains the values given and
keeps its others, so adding a person to `associated_people` keeps the people
already linked. A status attribute is refused with code `status_attribute`.

<!-- submilli:endif -->

<!-- submilli:if capability="attio.com/records.setStatus" -->

## Change a status

`setStatus(object, recordId, attribute, status)` moves a record to another
live status, such as a deal to another stage, Won and Lost included. `status`
is a title, matched without regard to case, or a status id. An unknown status
fails with code `unknown_status`, and the message lists the valid titles. The
result is a `StatusChange`: the `record`, the `status` title now set, the
`previousStatus` title (`""` when there was none), and `changed`, false when
the record already had that status and nothing was written.

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

- A missing record returns `null` from `getRecord` and `findPersonByEmail`.
- Other failures throw `AttioError` with `code`, `message` and `status`.
  `status` 0 is an error the package raised itself: invalid input such as an
  email, slug, record id, value or page size (refused before anything is
  sent), an unknown status, a status attribute in a plain write, or two
  people found with one email. Fix the argument rather than retrying. Otherwise `status`
  is Attio's HTTP status and `code` Attio's own, such as `value_not_found` for
  an unknown option or member.
- `retryable` is true when Attio did not apply the request: a rate limit
  (status 429, with the time to wait in `retryAfter`) or a concurrent write
  (`concurrent_write_conflict`). Nothing is retried automatically.
- A denied operation throws `PermissionDeniedError`: the operation is not
  allowed with those arguments, so do not retry it with other spellings.

<!-- submilli:if all="attio.com/companies.upsert attio.com/people.upsert attio.com/records.create attio.com/notes.create" -->

## Example

Record a new lead: the company by domain, the contact by email, a deal in the
Lead stage owned by a workspace member and linked to both, and a note with the
source text.

```ts
import { createNote, createRecord, upsertCompany, upsertPerson } from "@submilli/attio";

function main(): { deal: string; url: string } {
    const company = upsertCompany({ domain: "acme.com", name: "Acme" });
    const person = upsertPerson({ email: "dana@acme.com", firstName: "Dana", companyRecordId: company.recordId });
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
