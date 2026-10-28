// Live reads against the Attio workspace whose key is bound as ATTIO_API_KEY.
// Read-only: nothing here writes. Skipped by `--skip-network` and when the key
// is not bound.
import { label } from "submilli:test";
import secrets from "submilli:secrets";
import {
    findPersonByEmail,
    getRecord,
    listAttributes,
    listMembers,
    listNotes,
    listObjects,
    listStatuses,
    queryRecords,
} from "@submilli/attio";

const ABSENT_RECORD = "00000000-0000-4000-8000-000000000000";

function main(): void {
    if (secrets.get("ATTIO_API_KEY") === undefined) {
        label("skip: ATTIO_API_KEY is not bound");
        return;
    }

    label("objects, attributes and members");
    const objects = listObjects();
    let hasDeals = false;
    let hasPeople = false;
    for (const object of objects) {
        if (object.slug === "deals") hasDeals = true;
        if (object.slug === "people") hasPeople = true;
    }
    assert(hasPeople, "the people object exists in every workspace");
    const attributes = listAttributes("people");
    let hasEmail = false;
    for (const attribute of attributes) {
        if (attribute.slug === "email_addresses") hasEmail = attribute.type === "email-address" && attribute.isMultiselect;
    }
    assert(hasEmail, "people.email_addresses is a multi-value email attribute");
    assert(listMembers().length > 0, "the workspace has at least one member");

    label("missing records and people read as null");
    assert(getRecord("people", ABSENT_RECORD) === null, "absent record is null");
    assert(findPersonByEmail("nobody-" + ABSENT_RECORD + "@example.invalid") === null, "unknown email is null");

    label("a page of people, and one of them by id");
    const page = queryRecords("people", { limit: 2 });
    assert(page.items.length <= 2, "limit respected");
    assert(page.isComplete === (page.nextOffset === null), "isComplete matches nextOffset");
    if (page.items.length > 0) {
        const person = page.items[0];
        const fetched = getRecord("people", person.recordId);
        assert(fetched !== null && fetched.recordId === person.recordId, "listed person can be fetched");
        const notes = listNotes("people", person.recordId, { limit: 5 });
        for (const note of notes.items) assert(note.parentRecordId === person.recordId, "notes belong to the record");
    }

    if (!hasDeals) {
        label("skip: the deals object is not enabled in this workspace");
        return;
    }
    label("deal stages and a stage-filtered query");
    const stages = listStatuses("deals", "stage");
    assert(stages.length > 0, "deals.stage has live statuses");
    const inStage = queryRecords("deals", { filter: [{ attribute: "stage", operator: "eq", value: stages[0].title }], limit: 5 });
    for (const deal of inStage.items) {
        const stage = deal.values["stage"];
        assert(stage !== undefined && stage.length > 0 && stage[0].value === stages[0].title, "filtered deals are in that stage");
    }
}
