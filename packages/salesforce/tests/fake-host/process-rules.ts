// Run by `salesforce_process_rules_hold_against_a_fake_salesforce` in
// crates/submilli/tests/package_policy.rs. Each check runs with the facts the
// package read from a fake Salesforce: a record's owner and account, a lead's
// owner, an approval's object and process. The test also asserts which writes
// reached the fake.
import { approve, changeOwner, convertLead, logActivity, postToFeed, reject } from "@submilli/salesforce";

const OTHER = "005000000000002AAA";
const SALES_QUEUE = "00G000000000001EAA";
const MINE = "006000000000001AAA";
const THEIRS = "006000000000002AAA";
const MISSING = "006000000000003AAA";
const LEAD_MINE = "00Q000000000001EAA";
const LEAD_THEIRS = "00Q000000000002EAA";
const WORK_ITEM = "04i000000000001AAA";

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
    // Its own deal may go to a queue; another owner's deal, or a hand-off to a person, is denied.
    changeOwner("Opportunity", MINE, SALES_QUEUE);
    deniedAt("salesforce.com/records.changeOwner", "main", () => { changeOwner("Opportunity", THEIRS, SALES_QUEUE); });
    deniedAt("salesforce.com/records.changeOwner", "main", () => { changeOwner("Opportunity", MINE, OTHER); });

    // Its own leads, converted without an opportunity and kept by their owner.
    const converted = convertLead(LEAD_MINE, { createOpportunity: false });
    assert(converted.accountId === "001000000000001AAA" && converted.contactId === "003000000000001AAA", "converted");
    assert(converted.opportunityId === null, "no opportunity");
    deniedAt("salesforce.com/leads.convert", "main", () => { convertLead(LEAD_THEIRS, { createOpportunity: false }); });
    deniedAt("salesforce.com/leads.convert", "main", () => { convertLead(LEAD_MINE); });
    deniedAt("salesforce.com/leads.convert", "main", () => { convertLead(LEAD_MINE, { createOpportunity: false, ownerId: OTHER }); });

    // Calls on the account's records: the account is read from the deal, so a
    // missing deal checks with "" and is denied.
    logActivity({ type: "call", subject: "Discussed the renewal", whatId: MINE });
    deniedAt("salesforce.com/activities.log", "main", () => { logActivity({ type: "email", subject: "Sent terms", whatId: MINE }); });
    deniedAt("salesforce.com/activities.log", "main", () => { logActivity({ type: "call", subject: "Called", whatId: MISSING }); });

    // Discount approvals of opportunities: approve, never reject.
    const approved = approve(WORK_ITEM, { comment: "Within policy" });
    assert(approved.status === "Approved" && approved.recordId === MINE, "approved");
    deniedAt("salesforce.com/approvals.reject", "main", () => { reject(WORK_ITEM, { comment: "No" }); });

    // Posts on the account's records, without mentioning other people.
    postToFeed(MINE, "Renewal approved");
    deniedAt("salesforce.com/feed.post", "main", () => { postToFeed(MINE, "FYI", { mentions: [OTHER] }); });
    return "process rules hold against the fake Salesforce";
}
