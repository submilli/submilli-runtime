// Run by `salesforce_owner_rules_hold_against_a_fake_salesforce` in
// crates/submilli/tests/package_policy.rs. The package copy talks to a fake Salesforce, so
// each check runs with the owner the package read from it, under the real policy engine.
// The test also asserts which writes reached the fake.
import { deleteRecord, getRecord, updateOpportunity, updateRecord } from "@submilli/salesforce";

const MINE = "006000000000001AAA";
const THEIRS = "006000000000002AAA";
const MISSING = "006000000000003AAA";

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

function main(): string {
    // Reads: its own deal, by a 15-character id; another owner's is denied, and so is a
    // missing one, so a denial does not tell whether a record exists.
    const mine = getRecord("Opportunity", "006000000000001", { fields: ["Name", "stagename"] });
    assert(mine !== null && mine.fields["Name"] === "Acme pilot", "read its own deal");
    assert(mine !== null && mine.fields["stagename"] === "Prospecting", "fields as asked for");
    deniedAt("salesforce.com/records.get", "main", () => { getRecord("Opportunity", THEIRS); });
    deniedAt("salesforce.com/records.get", "main", () => { getRecord("Opportunity", MISSING); });

    // Writes: the owner comes from Salesforce, not from the caller.
    updateRecord("Opportunity", MINE, [{ field: "Name", value: "Acme renewal" }]);
    deniedAt("salesforce.com/records.update", "main", () => { updateRecord("Opportunity", THEIRS, [{ field: "Name", value: "Mine now" }]); });
    updateOpportunity(MINE, { stage: "Closed Won" });
    deniedAt("salesforce.com/opportunities.update", "main", () => { updateOpportunity(MINE, { amount: 5000 }); });
    deniedAt("salesforce.com/opportunities.update", "main", () => { updateOpportunity(THEIRS, { stage: "Closed Won" }); });
    deniedAt("salesforce.com/records.delete", "main", () => { deleteRecord("Opportunity", THEIRS); });
    assert(deleteRecord("Opportunity", MINE), "deleted its own deal");
    return "owner rules hold against the fake Salesforce";
}
