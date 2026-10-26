// Live writes against a disposable deal. Runs only with ATTIO_API_KEY,
// ATTIO_LIVE_MUTATIONS=true and ATTIO_TEST_DEAL_ID bound. The deal's name and
// stage are changed and restored; a note is added to it and stays, because the
// package has no delete operation. ATTIO_TEST_COMPANY_DOMAIN and
// ATTIO_TEST_PERSON_EMAIL, when bound, also upsert that company and person
// twice; those records stay too.
import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { AttioRecord, createNote, getRecord, listNotes, listStatuses, setStatus, updateRecord, upsertCompany, upsertPerson } from "@submilli/attio";

function firstValue(record: AttioRecord, slug: string): string {
    const values = record.values[slug];
    if (values === undefined || values.length === 0) return "";
    return values[0].value;
}

function main(): void {
    if (secrets.get("ATTIO_API_KEY") === undefined || secrets.get("ATTIO_LIVE_MUTATIONS") !== "true") {
        label("skip: Attio live mutations are disabled");
        return;
    }
    const dealId = secrets.get("ATTIO_TEST_DEAL_ID");
    if (dealId === undefined) {
        label("skip: ATTIO_TEST_DEAL_ID is not bound");
        return;
    }
    const deal = getRecord("deals", dealId);
    assert(deal !== null, "the disposable deal exists");
    if (deal === null) return;

    label("updateRecord changes only the name, then restores it");
    const name = firstValue(deal, "name");
    const renamed = updateRecord("deals", dealId, [{ attribute: "name", type: "text", text: name + " (submilli test)" }]);
    assert(firstValue(renamed, "name") === name + " (submilli test)", "name changed");
    assert(firstValue(renamed, "stage") === firstValue(deal, "stage"), "stage untouched");
    if (name.length > 0) {
        const restored = updateRecord("deals", dealId, [{ attribute: "name", type: "text", text: name }]);
        assert(firstValue(restored, "name") === name, "name restored");
    }

    label("setStatus moves the stage, reports the previous one, and restores it");
    const stages = listStatuses("deals", "stage");
    const original = firstValue(deal, "stage");
    let other = "";
    for (const stage of stages) {
        if (other.length === 0 && stage.title !== original) other = stage.title;
    }
    if (other.length > 0) {
        const moved = setStatus("deals", dealId, "stage", other.toLowerCase());
        assert(moved.changed && moved.status === other && moved.previousStatus === original, "stage moved by case-insensitive title");
        const again = setStatus("deals", dealId, "stage", other);
        assert(!again.changed && again.previousStatus === other, "same stage is not rewritten");
        if (original.length > 0) {
            const back = setStatus("deals", dealId, "stage", original);
            assert(back.changed && back.status === original, "stage restored");
        }
    }

    label("createNote attaches a note that listNotes returns");
    const note = createNote("deals", dealId, { title: "Submilli live test", content: "Created by packages/attio network_mutation.test.ts" });
    assert(note.parentRecordId === dealId.toLowerCase(), "note attached to the deal");
    const notes = listNotes("deals", dealId, { limit: 50 });
    let found = false;
    for (const item of notes.items) {
        if (item.noteId === note.noteId) found = true;
    }
    assert(found || !notes.isComplete, "the new note is listed on the first page, or more pages remain");

    const domain = secrets.get("ATTIO_TEST_COMPANY_DOMAIN");
    if (domain !== undefined) {
        label("upsertCompany matches the same company twice");
        const first = upsertCompany({ domain: domain });
        const second = upsertCompany({ domain: "https://www." + domain + "/" });
        assert(first.recordId === second.recordId, "normalized domain matches the same company");
    }
    const email = secrets.get("ATTIO_TEST_PERSON_EMAIL");
    if (email !== undefined) {
        label("upsertPerson matches the same person twice");
        const first = upsertPerson({ email: email });
        const second = upsertPerson({ email: " " + email.toUpperCase() + " " });
        assert(first.recordId === second.recordId, "normalized email matches the same person");
    }
}
