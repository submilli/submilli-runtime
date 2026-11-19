// Run through `submilli run --blueprint assistant-scope.yaml`, not the allow-all build test runner.
// The checks here come before any request, so each allowed call stops when the package
// reads the org's URL or its token, which the blueprint does not grant it.
import {
    describeObject,
    getLimits,
    listPendingApprovals,
    logActivity,
    runFlow,
    runReport,
    sendEmail,
} from "@submilli/salesforce";

const ME = "005000000000001AAA";

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

function main(): string {
    // Email only inside the company: every recipient at example.com, in any case.
    allowed(() => { sendEmail({ to: ["dana@example.com", "LEE@Example.com"], subject: "Notes", body: "Attached." }); });
    deniedAt("salesforce.com/email.send", "main", () => { sendEmail({ to: ["dana@example.com", "x@other.org"], subject: "Notes", body: "Hi" }); });
    deniedAt("salesforce.com/email.send", "main", () => { sendEmail({ to: ["x@other.org"], subject: "Notes", body: "Hi" }); });

    // One flow, by its API name in any case; no other.
    allowed(() => { runFlow("Escalate_Case", [{ name: "caseId", value: "500000000000001AAA" }]); });
    allowed(() => { runFlow("escalate_case"); });
    deniedAt("salesforce.com/flows.run", "main", () => { runFlow("Delete_Accounts"); });

    // The team's report, by a 15-character id too, with filters of its own.
    allowed(() => { runReport("00O000000000001", { filters: [{ column: "STAGE_NAME", operator: "equals", value: "Prospecting" }] }); });
    deniedAt("salesforce.com/reports.run", "main", () => { runReport("00O000000000002EAA"); });

    // Its own approvals queue only.
    allowed(() => { listPendingApprovals({ actorId: ME }); });
    deniedAt("salesforce.com/approvals.listPending", "main", () => { listPendingApprovals(); });

    // Calls logged on nothing yet; metadata reads; not the org's limits.
    allowed(() => { logActivity({ type: "call", subject: "Intro call" }); });
    deniedAt("salesforce.com/activities.log", "main", () => { logActivity({ type: "meeting", subject: "Lunch" }); });
    allowed(() => { describeObject("Opportunity"); });
    deniedAt("salesforce.com/org.limits", "main", () => { getLimits(); });
    return "the assistant's rules hold";
}
