// Run through `submilli run --blueprint deal-scope.yaml`, not the allow-all build test runner.
// The blueprint lets a leads agent write deals only: create and update them (but not
// their owner), move their stage, and attach notes to them. Records of other objects
// may be read, and companies upserted only for one domain.
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
    deniedAt("attio.com/records.create", "main", () => { createRecord("companies", { values: [{ attribute: "name", type: "text", text: "Acme" }] }); });

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
    reachesCredentialBoundary(() => { upsertPerson({ email: "dana@acme.com", firstName: "Dana" }); });

    // The domain rule sees the normalized domain, so a URL of the same company passes
    // and another company does not.
    reachesCredentialBoundary(() => { upsertCompany({ domain: "https://www.ACME.com/pricing" }); });
    deniedAt("attio.com/companies.upsert", "main", () => { upsertCompany({ domain: "victim.com" }); });
    deniedAt("attio.com/companies.upsert", "main", () => { upsertCompany({ domain: "acme.com.victim.com" }); });

    // A status change is checked with the canonical status title and the record's current
    // status, which only Attio knows. setStatus therefore reads the key and the statuses
    // before its check, so offline both an allowed and a denied attribute stop at the key.
    // The rule's decision on those fields is covered by scripts/contract.test.mjs.
    reachesCredentialBoundary(() => { setStatus("deals", DEAL, "stage", "Won"); });
    reachesCredentialBoundary(() => { setStatus("deals", DEAL, "priority", "High"); });
    // A status given at creation is resolved the same way before records.create is checked.
    reachesCredentialBoundary(() => { createRecord("companies", { status: { attribute: "stage", title: "Lead" } }); });
    return "deal scope checks passed";
}
