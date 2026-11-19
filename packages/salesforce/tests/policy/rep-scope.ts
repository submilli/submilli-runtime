// Run through `submilli run --blueprint rep-scope.yaml`, not the allow-all build test runner.
// The checks here come before any request, so each allowed call stops when the package
// reads the org's URL, which the blueprint does not grant it.
import {
    aggregate,
    createCase,
    createRecord,
    findAccounts,
    findOpportunities,
    query,
    SalesforceError,
    search,
    soql,
    updateOpportunity,
    updateRecord,
} from "@submilli/salesforce";

const ME = "005000000000001AAA";
const SOMEONE_ELSE = "005000000000002AAA";

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

function allowed(action: () => void): void {
    // This denial proves the business check passed, without a credential or a request.
    deniedAt("secrets.get", "@submilli/salesforce", action);
}

function rejectedAs(code: string, action: () => void): void {
    let outcome = "no error";
    try {
        action();
    } catch (error: SalesforceError) {
        outcome = error.errorCode;
    } catch (error) {
        outcome = error.name + ": " + error.message;
    }
    assert(outcome === code, "expected " + code + ", got " + outcome);
}

function main(): string {
    // Own pipeline only: a 15-character id checks as its 18-character form; another
    // owner, or no owner filter at all, is denied.
    allowed(() => { findOpportunities({ ownerId: ME, isClosed: false }); });
    allowed(() => { findOpportunities({ ownerId: "005000000000001" }); });
    deniedAt("salesforce.com/opportunities.find", "main", () => { findOpportunities({ ownerId: SOMEONE_ELSE }); });
    deniedAt("salesforce.com/opportunities.find", "main", () => { findOpportunities({ stage: "Prospecting" }); });
    allowed(() => { findAccounts({ ownerId: ME }); });
    deniedAt("salesforce.com/accounts.find", "main", () => { findAccounts({ name: "Acme" }); });

    // Queries of opportunities, without reading, filtering or sorting on Amount in any case.
    allowed(() => { query({ object: "Opportunity", fields: ["Name", "StageName"] }); });
    allowed(() => { query({ object: "opportunity", fields: ["Name"], where: [{ field: "StageName", op: "=", value: "Prospecting" }] }); });
    deniedAt("salesforce.com/query.run", "main", () => { query({ object: "Opportunity", fields: ["AMOUNT"] }); });
    deniedAt("salesforce.com/query.run", "main", () => {
        query({ object: "Opportunity", fields: ["Name"], where: [{ field: "Amount", op: ">", value: 100000 }] });
    });
    deniedAt("salesforce.com/query.run", "main", () => {
        query({ object: "Opportunity", fields: ["Name"], orderBy: [{ field: "Amount", direction: "desc" }] });
    });
    deniedAt("salesforce.com/query.run", "main", () => { query({ object: "Account", fields: ["Name"] }); });
    allowed(() => { aggregate({ object: "Opportunity", groupBy: [{ field: "StageName" }], measures: [{ fn: "count" }] }); });
    deniedAt("salesforce.com/query.aggregate", "main", () => { aggregate({ object: "Case", measures: [{ fn: "count" }] }); });

    // Search, but not of users.
    allowed(() => { search({ text: "Acme", objects: [{ object: "Account" }, { object: "Contact" }] }); });
    deniedAt("salesforce.com/query.search", "main", () => { search({ text: "Dana", objects: [{ object: "User" }] }); });

    // Tasks it owns, and cases without assigning them.
    allowed(() => { createRecord("Task", [{ field: "Subject", value: "Call Acme" }]); });
    deniedAt("salesforce.com/records.create", "main", () => {
        createRecord("Task", [{ field: "Subject", value: "Call Acme" }, { field: "OwnerId", value: SOMEONE_ELSE }]);
    });
    deniedAt("salesforce.com/records.create", "main", () => { createRecord("Account", [{ field: "Name", value: "Acme" }]); });
    allowed(() => { createCase({ subject: "Login fails" }); });
    deniedAt("salesforce.com/cases.create", "main", () => { createCase({ subject: "Login fails", ownerId: SOMEONE_ELSE }); });

    // No raw SOQL, and no owner change through a plain update, whatever the blueprint allows.
    deniedAt("salesforce.com/query.soql", "main", () => { soql("SELECT Amount FROM Opportunity"); });
    rejectedAs("owner_field", () => { updateRecord("Opportunity", "006000000000001", [{ field: "OwnerId", value: ME }]); });
    // A stage rule cannot be bypassed through custom, nor an account rule by moving the record.
    rejectedAs("typed_field", () => { updateOpportunity("006000000000001", { custom: [{ field: "StageName", value: "Closed Won" }] }); });
    rejectedAs("account_field", () => { updateRecord("Contact", "003000000000001", [{ field: "AccountId", value: "001000000000001" }]); });
    return "the rep's rules hold";
}
