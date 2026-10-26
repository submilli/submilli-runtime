# @submilli/attio

A synchronous, typed client for the [Attio](https://attio.com) CRM REST API:
object, attribute and status discovery, workspace members, typed record
queries, people and company upserts by email and domain, record creation and
updates, status (deal stage) changes, and notes. It has no delete operations
and no operations that change the workspace schema.

## Blueprint setup

Add the package and bind an Attio API key as `ATTIO_API_KEY`:

```yaml
packages:
  - "@submilli/attio"

secrets:
  ATTIO_API_KEY:
    store: ATTIO_API_KEY
```

Generate a workspace API key in Attio's developer settings (**Workspace
settings → Developers**; see Attio's help article
[Generating an API key](https://attio.com/help/apps/other-apps/generating-an-api-key))
with these scopes:

| Scope | Needed by |
| --- | --- |
| `object_configuration:read` | every operation except `listMembers` (objects, attributes and statuses are resolved by slug) |
| `record_permission:read-write` | record reads, queries, upserts, creates, updates and status changes (`record_permission:read` is enough for a read-only blueprint) |
| `note:read-write` | `createNote` and `listNotes` (`note:read` for `listNotes` alone) |
| `user_management:read` | `listMembers` |

The package reads the key with `secrets.get("ATTIO_API_KEY")` and sends it as
`Authorization: Bearer`. It calls only `https://api.attio.com/v2` with GET,
POST, PUT and PATCH, never DELETE. `submilli blueprint add-package
@submilli/attio` writes those grants from `capabilities.yaml`:

```yaml
permissions:
  '@submilli/attio':
    - capability: http.get
      filter: host == "api.attio.com"
      action: allow
    - capability: http.patch
      filter: host == "api.attio.com"
      action: allow
    - capability: http.post
      filter: host == "api.attio.com"
      action: allow
    - capability: http.put
      filter: host == "api.attio.com"
      action: allow
    - capability: secrets.get
      filter: name == "ATTIO_API_KEY"
      action: allow
```

### Enabling Deals

The standard `deals` object exists in every workspace but is disabled by
default. A workspace admin activates it on Attio's
[objects settings](https://app.attio.com/_/settings/data/objects) page
(**Workspace settings → Objects**). Its attributes are `name` (text,
required), `stage` (status, required; the default stages are Lead,
In Progress, Won 🎉 and Lost, and admins can rename them), `owner` (workspace
member, required), `value` (currency, USD by default), `associated_company`
and `associated_people` (record references). A deal is therefore created with
a name, an owner given as a workspace member's email, and a status.
`listObjects` shows whether deals are enabled, and
`listStatuses("deals", "stage")` shows the live stage titles.

## Upserts overwrite existing records

`upsertPerson` and `upsertCompany` create the record when no person has the
email (or no company has the domain), and otherwise **update the existing
record: every field the call gives replaces that record's current value**.
`upsertPerson({ email, firstName: "Dana" })` renames whoever in the workspace
already has that email; `upsertCompany({ domain, name })` renames the company
that owns the domain. The check's `attributes` field lists what the call
writes (`email_addresses` or `domains`, plus `name`, `job_title`,
`description`, `company`), so a blueprint can allow an upsert that only links
or creates and refuse one that would overwrite:

```yaml
    - capability: attio.com/companies.upsert
      filter: not (attributes contains "name") and not (attributes contains "description")
      action: allow
```

Fields the call leaves out are not sent and keep their values, and the
record's other email addresses or domains stay.

## Capabilities

Every check field is normalized before the check and sent exactly as checked.
Object and attribute fields are lowercase api_slugs: an object or attribute
UUID is refused before the check, so a rule such as `object == "deals"` cannot
be bypassed with the object's id. Record ids are lowercase UUIDs.

| Capability | Operation | Fields |
| --- | --- | --- |
| `attio.com/objects.list` | `listObjects()` | none |
| `attio.com/attributes.list` | `listAttributes(object)` | `object` |
| `attio.com/statuses.list` | `listStatuses(object, attribute)` | `object`, `attribute` |
| `attio.com/members.list` | `listMembers()` | none |
| `attio.com/records.get` | `getRecord(object, recordId)` | `object`, `recordId` |
| `attio.com/records.query` | `queryRecords(object, query)` | `object` |
| `attio.com/people.find` | `findPersonByEmail(email)` | `email` (trimmed, lowercased) |
| `attio.com/people.upsert` | `upsertPerson(input)` | `email` (trimmed, lowercased), `attributes` |
| `attio.com/companies.upsert` | `upsertCompany(input)` | `domain` (lowercased ASCII; no scheme, `www.`, port or path), `attributes` |
| `attio.com/records.create` | `createRecord(object, input)`; with a status, also `statuses.list` first | `object`, `attributes`, `statusAttribute`, `status` |
| `attio.com/records.update` | `updateRecord(object, recordId, values)` | `object`, `recordId`, `attributes` |
| `attio.com/records.setStatus` | `setStatus(object, recordId, attribute, status)`, after `statuses.list` and `records.get` | `object`, `recordId`, `attribute`, `status`, `previousStatus` |
| `attio.com/notes.create` | `createNote(parentObject, parentRecordId, input)` | `parentObject`, `parentRecordId` |
| `attio.com/notes.list` | `listNotes(parentObject, parentRecordId, page)` | `parentObject`, `parentRecordId` |

- `attributes` is the sorted list of distinct attribute slugs the call writes
  (a `string[]`; filter it with `contains`). For `createRecord` the initial
  status attribute is not in it: it is reported in `statusAttribute`. For the
  upserts it includes the matching attribute (`email_addresses`, `domains`).
- Domains must be ASCII: an internationalized domain is refused unless it is
  given in punycode (`xn--…`), so a `domain` rule compares the spelling Attio
  stores.
- Status attributes, such as a deal's `stage`, change only through
  `setStatus`, or through `createRecord`'s `status`. `updateRecord` and the
  values of `createRecord` look up the object's attribute types in Attio and
  refuse a status attribute, so a stage rule cannot be bypassed with a plain
  update.
- `status` is the canonical title of a live (non-archived) status, resolved
  from the title (case-insensitive) or status id the program gave.
  `previousStatus` is the record's current title, read from Attio by the
  package (`""` when it has none; it may be an archived status). `statusAttribute`
  and `status` are `""` when `createRecord` sets no status.
- `setStatus` and `createRecord` with a status need Attio's answers for their
  check, so they check each read first, as the read operation it is:
  `statuses.list { object, attribute }` before reading the statuses, and, for
  `setStatus`, `records.get { object, recordId }` before reading the record.
  **Granting `setStatus` therefore also needs `records.get` and
  `statuses.list` on that object, and `createRecord` with a status needs
  `statuses.list`.** A caller who may not read the record is denied before the
  read, so a denial reveals neither its current status nor whether it exists.
  Nothing is written unless the final check passes. The other operations check
  before any request.
- Returned records show actor references, such as a deal's `owner`, as the
  workspace member id; the package does not look members up. `listMembers`
  maps ids to emails.
- Values written, note text, query conditions and page options are not check
  fields.

A blueprint for an agent that records leads as deals and moves them through
any stage, without touching other objects' records:

```yaml
permissions:
  main:
    - capability: attio.com/objects.list
      action: allow
    - capability: attio.com/attributes.list
      action: allow
    - capability: attio.com/statuses.list
      action: allow
    - capability: attio.com/members.list
      action: allow
    - capability: attio.com/records.get
      filter: object == "deals" or object == "companies" or object == "people"
      action: allow
    - capability: attio.com/records.query
      filter: object == "deals" or object == "companies" or object == "people"
      action: allow
    - capability: attio.com/people.find
      action: allow
    - capability: attio.com/people.upsert
      action: allow
    - capability: attio.com/companies.upsert
      filter: not (attributes contains "name") and not (attributes contains "description")
      action: allow
    - capability: attio.com/records.create
      filter: object == "deals"
      action: allow
    - capability: attio.com/records.update
      filter: object == "deals"
      action: allow
    - capability: attio.com/records.setStatus
      filter: object == "deals" and attribute == "stage"
      action: allow
    - capability: attio.com/notes.create
      filter: parentObject == "deals"
      action: allow
    - capability: attio.com/notes.list
      filter: parentObject == "deals"
      action: allow
```

Narrow it further with fields such as `not (attributes contains "owner")` or
`status != "Lost"`.

## Write semantics

- `updateRecord` uses `PATCH /v2/objects/{object}/records/{id}`: only the
  attributes given are sent; single-value attributes are replaced and
  multi-value attributes gain the given values (Attio prepends them) without
  losing their others.
- `upsertPerson` and `upsertCompany` (see
  [Upserts overwrite existing records](#upserts-overwrite-existing-records))
  use Attio's assert endpoint
  (`PUT /v2/objects/{object}/records?matching_attribute=…`) matched on
  `email_addresses` and `domains`. For the matching attribute Attio adds the
  value and keeps the existing ones. Attio overwrites every *other*
  multi-value attribute sent in an upsert, so the package sends only the
  matching value and the single-value fields the program gave (`name`,
  `job_title`, `description`, `company` for people; `name`, `description` for
  companies). Fields the program leaves out are not sent and keep their
  values. When several records match, Attio rejects the upsert and the package
  throws `multiple_match_results` naming the email or domain.
- `setStatus` writes the status id with the same PATCH, never through Attio's
  beta attribute-values endpoint, so the stage history is kept. When the
  record already has the status, nothing is written and `changed` is false.
- `createNote` uses `POST /v2/notes`. `listNotes` always sends both
  `parent_object` and `parent_record_id` (Attio lists notes across the whole
  workspace without them) and drops any note whose parent object or record
  differs.
- Nothing is retried automatically. `AttioError.retryable` marks a 429 (with
  `retryAfter`, Attio's `Retry-After` HTTP date) and a 409
  `concurrent_write_conflict`, which Attio did not apply.

Queries and note lists return one page (`items`, `nextOffset`,
`isComplete`); the package never follows pages on its own.

## Development

```sh
cargo run -p submilli -- build test --deny-warnings -p @submilli/attio --skip-network
node --test packages/attio/scripts/contract.test.mjs
cargo test -p submilli --test package_policy
```

The Node harness never evaluates a blueprint filter: its `check` records the
context. Rule decisions are exercised by the policy tests below.

`tests/lib.test.ts` runs offline: normalizers, request builders, response
mappers, and input validation before any check or request. The Node contract
test (Node 22.13+) runs the real operations against a mocked Submilli host and
a mocked Attio API: the requests each operation sends, pagination, 429 and 409
handling, `null` on 404, ambiguous upserts, status refusal in `updateRecord`,
status resolution in `setStatus`, and that `check()` receives the values the
request uses.

Live tests run only with credentials, never under `--skip-network`:

- `tests/network_read.test.ts` needs `ATTIO_API_KEY` and only reads.
- `tests/network_mutation.test.ts` also needs `ATTIO_LIVE_MUTATIONS=true` and
  `ATTIO_TEST_DEAL_ID`, a disposable deal. It renames the deal and moves its
  stage, then restores both, and adds a note that stays (the package cannot
  delete). `ATTIO_TEST_COMPANY_DOMAIN` and `ATTIO_TEST_PERSON_EMAIL` add
  upsert checks; those records stay too.

```sh
cargo run -p submilli -- build test -p @submilli/attio --env-var ATTIO_API_KEY
```

## Policy tests

The scripts in `tests/policy/` run as a real `main` caller under restricted
blueprints of the same name, without a key or network:

- `deal-scope.ts` shows deal writes and deal notes allowed while the same
  writes to companies, an `owner` change, notes on people and queries of other
  objects are denied; that an object UUID is refused before the check; and that
  the company rule sees the normalized domain. It also shows `setStatus` and a
  creation status denied at `statuses.list` on another object; on deals they
  stop at the key, because the `setStatus` check needs Attio's answers.
- `read-once.ts` shows that an attribute slug, a domain or an email whose
  getter answers differently on a second read is read once, so the value the
  rule approved is the value sent.

`attio_status_rules_hold_against_a_fake_attio` in
`crates/submilli/tests/package_policy.rs` covers what needs Attio's answers. It
publishes a copy of the package whose API base points at a fake Attio on
`127.0.0.1` and runs `tests/fake-host/status-rules.ts` under its blueprint: a
stage change is allowed and written, a change of another status attribute is
denied and not written, and a status attribute among plain values is refused.
