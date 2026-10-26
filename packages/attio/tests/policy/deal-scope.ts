// Run through `submilli run --blueprint deal-scope.yaml`, not the allow-all build test runner.
// The blueprint lets a leads agent change deals only: create and update them (but not
// their owner), move their stage, and attach notes to them. Companies and people may be
// created but not upserted, records of the three objects read, and one company domain
// upserted without renaming it.
import {
    AttioError,
    createNote,
    createRecord,
    findPersonByEmail,
    getRecord,
    listNotes,
    queryRecords,
    setStatus,
    updateRecord,
    upsertCompany,
    upsertPerson,
} from "@submilli/attio";

const DEAL = "8a1b2c3d-1111-4222-8333-944455556666";
const COMPANY = "0c0ffee0-aaaa-4bbb-8ccc-ddddeeeeffff";
const PERSON = "5e5e5e5e-1111-4222-8333-944455556666";
const DEALS_OBJECT_ID = "97052eb9-e65e-443f-a297-f2d9a4a7f795";

function deniedAt(capability: string, caller: string, action: () => void): void {
    let outcome = "no error";
    try {
        action();
    } catch (error: PermissionDeniedError) {
        outcome = error.caller + " denied at " + error.capability;
    } catch (error) {
        outcome = error.name + ": " + error.message;
    }
    const expected = caller + " denied at " + capability;
    assert(outcome === expected, "expected " + expected + ", got " + outcome);
}

function reachesCredentialBoundary(action: () => void): void {
    // This denial proves the business check passed, without reading a key or calling Attio.
    deniedAt("secrets.get", "@submilli/attio", action);
}

function rejectedAs(code: string, action: () => void): void {
    let outcome = "no error";
    try {
        action();
    } catch (error: AttioError) {
        outcome = error.code;
    } catch (error) {
        outcome = error.name + ": " + error.message;
    }
    assert(outcome === code, "expected " + code + ", got " + outcome);
}

function main(): string {
    // Deal writes are allowed; the same writes to companies are not.
    reachesCredentialBoundary(() => { updateRecord("deals", DEAL, [{ attribute: "name", type: "text", text: "Acme pilot" }]); });
    reachesCredentialBoundary(() => { updateRecord(" Deals ", DEAL, [{ attribute: "Name", type: "text", text: "Acme pilot" }]); });
    deniedAt("attio.com/records.update", "main", () => { updateRecord("companies", DEAL, [{ attribute: "name", type: "text", text: "Acme" }]); });
    deniedAt("attio.com/records.update", "main", () => {
        updateRecord("deals", DEAL, [
            { attribute: "name", type: "text", text: "Acme pilot" },
            { attribute: "owner", type: "member", email: "someone@example.com" },
        ]);
    });
    // An object id instead of the slug cannot slip past an `object == "deals"` rule.
    rejectedAs("invalid_slug", () => { updateRecord(DEALS_OBJECT_ID, DEAL, [{ attribute: "name", type: "text", text: "x" }]); });
    reachesCredentialBoundary(() => { createRecord("deals", { values: [{ attribute: "name", type: "text", text: "Acme pilot" }] }); });
    deniedAt("attio.com/records.create", "main", () => { createRecord("workspaces", { values: [{ attribute: "name", type: "text", text: "Acme" }] }); });
    // Companies and people are create-only: no upsert, which could overwrite an existing
    // record, is granted, and a create may write only attributes on the allow-list.
    reachesCredentialBoundary(() => {
        createRecord("companies", { values: [{ attribute: "domains", type: "domain", domain: "acme.com" }, { attribute: "name", type: "text", text: "Acme" }] });
    });
    reachesCredentialBoundary(() => { createRecord("companies", { values: [{ attribute: "domains", type: "domain", domain: "acme.com" }] }); });
    reachesCredentialBoundary(() => {
        createRecord("people", { values: [
            { attribute: "email_addresses", type: "email", email: "dana@acme.com" },
            { attribute: "name", type: "personal-name", firstName: "Dana" },
            { attribute: "company", type: "record", targetObject: "companies", targetRecordId: COMPANY },
        ] });
    });
    // `team` is the inverse of each person's single `company`: a new company listing an
    // existing person would move that person. Not on the allow-list, so denied, as is
    // `associated_deals`, which would move deals, and any attribute beyond the list.
    deniedAt("attio.com/records.create", "main", () => {
        createRecord("companies", { values: [
            { attribute: "domains", type: "domain", domain: "acme.com" },
            { attribute: "team", type: "record", targetObject: "people", targetRecordId: PERSON },
        ] });
    });
    deniedAt("attio.com/records.create", "main", () => {
        createRecord("companies", { values: [
            { attribute: "associated_deals", type: "record", targetObject: "deals", targetRecordId: DEAL },
            { attribute: "domains", type: "domain", domain: "acme.com" },
        ] });
    });
    deniedAt("attio.com/records.create", "main", () => {
        createRecord("people", { values: [
            { attribute: "email_addresses", type: "email", email: "dana@acme.com" },
            { attribute: "associated_users", type: "record", targetObject: "users", targetRecordId: PERSON },
        ] });
    });
    // A create with a status is checked at statuses.list first, which this blueprint grants
    // on deals only; the fake-host test shows records.create itself refusing it.
    deniedAt("attio.com/statuses.list", "main", () => {
        createRecord("people", { values: [{ attribute: "email_addresses", type: "email", email: "dana@acme.com" }], status: { attribute: "stage", title: "Lead" } });
    });

    // Notes only on deals.
    reachesCredentialBoundary(() => { createNote("deals", DEAL, { title: "Met at meetup", content: "Wants a demo" }); });
    deniedAt("attio.com/notes.create", "main", () => { createNote("people", DEAL, { title: "Met", content: "x" }); });
    reachesCredentialBoundary(() => { listNotes("deals", DEAL); });
    deniedAt("attio.com/notes.list", "main", () => { listNotes("companies", DEAL); });

    // Reads of the three objects, normalized before the rule sees them.
    reachesCredentialBoundary(() => { getRecord("DEALS", DEAL); });
    reachesCredentialBoundary(() => { queryRecords("people", { limit: 1 }); });
    deniedAt("attio.com/records.query", "main", () => { queryRecords("workspaces"); });
    reachesCredentialBoundary(() => { findPersonByEmail(" Dana@Acme.com "); });
    deniedAt("attio.com/people.upsert", "main", () => { upsertPerson({ email: "dana@acme.com", firstName: "Dana" }); });

    // The upsert rule sees the normalized domain and the attributes written, so a URL of
    // the same company that writes only the domain passes, and another company or an
    // upsert that would rename an existing one does not.
    reachesCredentialBoundary(() => { upsertCompany({ domain: "https://www.ACME.com/pricing" }); });
    deniedAt("attio.com/companies.upsert", "main", () => { upsertCompany({ domain: "acme.com", name: "Acme" }); });
    deniedAt("attio.com/companies.upsert", "main", () => { upsertCompany({ domain: "victim.com" }); });
    deniedAt("attio.com/companies.upsert", "main", () => { upsertCompany({ domain: "acme.com.victim.com" }); });

    // setStatus checks statuses.list before it reads the statuses and records.get before it
    // reads the record, so on an object whose statuses the caller may not list it is denied
    // before any read. A status given to createRecord is checked the same way.
    deniedAt("attio.com/statuses.list", "main", () => { setStatus("companies", DEAL, "stage", "Won"); });
    deniedAt("attio.com/statuses.list", "main", () => { createRecord("companies", { status: { attribute: "stage", title: "Lead" } }); });
    // On deals both reads are allowed, and the records.setStatus check itself needs the
    // canonical title and the current status that only Attio knows. Offline both an allowed
    // and a forbidden attribute therefore stop at the key. The records.setStatus rule's
    // decision is exercised against a fake Attio by `attio_status_rules_hold_against_a_fake_attio`
    // in crates/submilli/tests/package_policy.rs.
    reachesCredentialBoundary(() => { setStatus("deals", DEAL, "stage", "Won"); });
    reachesCredentialBoundary(() => { setStatus("deals", DEAL, "priority", "High"); });
    return "deal scope checks passed";
}
