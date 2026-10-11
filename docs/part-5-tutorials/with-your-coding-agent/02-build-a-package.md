---
title: "Build a Package"
description: "Have your coding assistant build a read-only Package over a real API: the Package, its readme, tests, a Blueprint, and a verifier's review, tested against the live service; then tests for a Package on a machine without the key. What a good result looks like."
slug: tutorials/build-a-package
# Re-run after SUB-1300 (https://linear.app/submilli/issue/SUB-1300): the
# skill should put live tests in `network.test.ts` files and suggest
# `--skip-network` without being asked. The files table and the "tests
# without the key" step change then.
sidebar:
  order: 2
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "63e9d13a366a06bc15436d74fd4155fa0c7eeeb4d6f6d4d90154687fbeb27da3"
  confirmedAt: "2026-10-09T15:40:52.000Z"
---

The Package pages showed how a Package is written by hand. With the
Submilli skill, your coding assistant builds one from a service's API
documentation. It writes the Package, the readme the model reads, the
tests, and a Blueprint, and it tests all of them against the service. Runs vary by
model, so each step below says what to look for, not what the assistant
will type.

In this tutorial we will have the assistant build a read-only Package
over a real API with a key, read its work, and then ask for tests on a
machine that has no key. The run was made with Claude Code, the skill,
and an [Attio](https://attio.com) workspace. Attio is a CRM, the kind of
service a support agent needs. Substitute your service and its key. You need the
skill from [Install](/docs/install#the-skill) and an empty project
directory.

## Put the key where the assistant can use it

The assistant needs the service's API key to test against it. Put the
key in an environment variable, or in a `.env` file beside where
`submilli.toml` will be, and tell the assistant which one. The key never
has to appear in the conversation:

```sh title=".env"
ATTIO_API_KEY=…
```

## Ask for the Package

```text
Build a read-only Submilli package over the Attio REST API (https://docs.attio.com/rest-api/overview) for our support agent. The agent may look up the company it is serving and read that company's people, notes, and tasks. It must never read any other company's records. The Attio API key is in the ATTIO_API_KEY environment variable.
```

Notice what it does before it writes. It reads Attio's reference pages
for the four endpoints it needs and samples the workspace to see the real
response shapes. Then it scaffolds the project and produces:

| File | Holds |
| --- | --- |
| `src/lib.ts` | `getCompany`, `listPeople`, `listNotes`, `listTasks`, each checking `{ companyId }` |
| `docs/readme.md` | What the model reads: each operation, paging, and that a denial means the record is forbidden, so don't retry with other ids |
| `tests/lib.test.ts` | Unit tests of the request builders, and live reads that skip without the key |
| `blueprint.yaml` | A required `companyId` variable, and one rule per operation |
| `verify.sh` | Programs run under the Blueprint: the allowed reads and each way the rule should refuse |

## Read one operation

`src/lib.ts` is about 500 lines. Most of it describes Attio's response
shapes and turns them into the four small types the model sees. Here is
one of those types and the operation that returns it:

```typescript title="src/lib.ts (fragment)"
/** A note attached directly to the company record. */
export interface Note {
    /** Note id (UUID). */
    id: string;
    /** Note title. */
    title: string;
    /** Body as Markdown. */
    content: string;
    /** ISO 8601 creation time. */
    createdAt: string;
}

/**
 * List notes attached directly to this company, as the Attio API orders them.
 * Notes attached to the company's people are not included.
 * @param companyId The company's record id.
 * @param page Page size and offset; omit it for the first page.
 * @returns One page of notes.
 * @capability attio.com/notes.list { companyId: string }
 */
export function listNotes(companyId: string, page?: PageOptions): Page<Note> {
    const limit = page?.limit;
    const offset = page?.offset;
    const id = normalizeRecordId(companyId);
    check("attio.com/notes.list", { companyId: id });
    const paging = resolvePage(limit, offset);
    const response = request("GET", "/notes" + buildNotesQuery(id, paging), null);
    if (response.status === 404) {
        // Attio answers 404 when the parent record does not exist.
        return { items: [], nextOffset: null };
    }
    const notes = (JSON.parse(response.body) as ListEnvelope<RawNote>).data;
    const items: Note[] = [];
    for (const note of notes) {
        if (note.parent_object === "companies" && note.parent_record_id === id) {
            items.push({
                id: note.id.note_id,
                title: note.title,
                content: note.content_markdown,
                createdAt: note.created_at,
            });
        }
    }
    return { items: items, nextOffset: nextOffset(paging, notes.length) };
}
```

Four things to check in any operation it writes, all visible here. The
id is normalized before the check, and the normalized id is the one
checked and the one sent, so the rule sees what Attio receives.
Each paging field is read once, before the check, and the helper takes
the fields and not the object the program passed. The loop keeps only
notes whose parent is that company, so a response that somehow names
another company's note doesn't reach the program. And the Package
defines `Note`, so the program never sees Attio's field names.

The rule it wrote covers all four operations with one filter, because
every operation carries the company:

```yaml title="blueprint.yaml (fragment)"
permissions:
  main:
  - capability: attio.com/notes.list
    filter: companyId == ${vars.companyId}
    action: allow
```

Two details weren't in the prompt. The first came from the service.
Attio's notes and tasks endpoints return every record in the workspace
when their company filter is missing, so `normalizeRecordId` refuses
anything but a record id, and each result's owner is checked before it
is returned, as `listNotes` does above. The second was the assistant's
choice. It narrowed the Package's grant from all of `api.attio.com` to
the paths it calls.

## Read the tests it ran

It tested against the live workspace. `submilli build test` passed,
live reads included. Under the Blueprint, a session bound to one company
read that company, its people, notes, and tasks. All four operations
were refused for another company, and calling Attio directly, reading
the key, and a session with no company were refused too. Two controls
showed the rule was the reason. Bound to the other company, the results
reversed, and with the filter removed, the other company's data came
through.

The skill has a second agent review the work, and in this run it did so
the way an attacker would, with ids in other forms, look-alike
characters, and a program that catches a denial and carries on. It found
no way to another company's records, and its findings led to four fixes,
among them leaving out a task linked to two companies.

Notice that the report ends with decisions for you: whether tasks shared
with another company should show, that notes on the company's people
aren't included, and that the application must bind `companyId` as the
company's record id. The workspace had no notes or tasks yet, so it said
that filtering them was tested only on sample data. The run took about
twelve minutes.

## Ask for tests without the key

Now the same skill on a machine that has no key, with the billing
Package from [Packages](/docs/packages/write-tests), which has one
test:

```text
Add tests for @acme/billing. I don't have the billing API key on this machine.
```

The assistant reads the Package and runs the existing test before
writing any. Then it writes tests that need no key: the invoice path for
a hostile id, `cus/../admin?x=1`, which must stay inside the customer's
segment of the path; credits for a premium and a standard customer; a
zero and a negative amount, refused before the permission check; and a
read with no key, which must fail naming `BILLING_API_KEY`. At the end
of the same file it adds a live read that runs only when the key is set.

Notice what it does with its own mistake. Its first expectation for the
hostile path didn't match what `encodeComponent` returns. It ran the
function to see, decided the Package was right and its test wrong, fixed
the test, and said so in its report. All the tests passed, about a
minute after the prompt.

The report separates what ran from what didn't. The live read hadn't
run, so the report calls it unverified until someone runs it with the
key. It also repeats the limit from [Write tests](/docs/packages/write-tests).
The tests show the Package works, not that a Blueprint refuses what it
should.

## Packages that write

This example is read-only on purpose. A Package that writes gets tested
by writing, so where the assistant does that matters. If the service has
a sandbox or test mode, give it a key for that. If it doesn't, tell it
which records it may change, such as one test company, and that it must
not touch anything else. Without that, a careful assistant tests writes
only with unit tests and says so. A less careful one writes to your real
data.

You have seen a Package built from an API reference and tested against
the service from both sides, read the four things that make an operation
safe, and watched the assistant correct itself and say what it didn't
prove. Next: [Connect a harness](/docs/tutorials/connect-a-harness).
