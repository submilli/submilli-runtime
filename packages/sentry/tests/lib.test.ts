// Query and body builders, issue-ID parsing and response normalizers are private:
// these tests reach them through the public operations. Invalid input fails with
// a local SentryError (status 0) before any request. Valid input gets past
// validation and stops at the token read with `missing_token`, because tests have
// no credentials by default; that segment is skipped when SENTRY_AUTH_TOKEN is
// bound, so this offline file never sends a request.
import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { SentryError, getIssue, listIssues, listOrganizations, updateIssue } from "@submilli/sentry";

function expectCode(code: string, action: () => void): void {
    let matched = false;
    try {
        action();
    } catch (cause) {
        if (cause instanceof SentryError) matched = cause.code === code && cause.status === 0;
    }
    assert(matched, "expected SentryError code " + code);
}

function main(): void {
    label("issue listing validates page size and time range");
    expectCode("invalid_page_size", () => { listIssues("pied-piper", { limit: 0 }); });
    expectCode("invalid_page_size", () => { listIssues("pied-piper", { limit: 4.5 }); });
    expectCode("invalid_page_size", () => { listOrganizations({ limit: 101 }); });
    expectCode("invalid_time_range", () => {
        listIssues("pied-piper", { statsPeriod: "24h", start: "2026-01-01T00:00:00Z", end: "2026-01-02T00:00:00Z" });
    });
    expectCode("invalid_time_range", () => { listIssues("pied-piper", { start: "2026-01-01T00:00:00Z" }); });

    label("issue references that are neither numeric nor short IDs are rejected");
    expectCode("invalid_issue_id", () => { getIssue("pied-piper", "backend", "not-an-id"); });
    expectCode("invalid_issue_id", () => { getIssue("pied-piper", "backend", "BACKEND-"); });

    label("triage updates are validated before any request");
    expectCode("empty_update", () => { updateIssue("pied-piper", "backend", "1", {}); });
    expectCode("invalid_update", () => { updateIssue("pied-piper", "backend", "1", { assignedTo: "user:1", clearAssignee: true }); });
    expectCode("invalid_update", () => { updateIssue("pied-piper", "backend", "1", { substatus: "archived_forever" }); });
    expectCode("invalid_update", () => { updateIssue("pied-piper", "backend", "1", { status: "unresolved", substatus: "archived_forever" }); });

    if (secrets.get("SENTRY_AUTH_TOKEN") !== undefined) {
        label("skip: valid input reaches the token read only when SENTRY_AUTH_TOKEN is not bound");
        return;
    }
    label("an unbound auth token is reported before any request");
    expectCode("missing_token", () => { listOrganizations(); });
    expectCode("missing_token", () => { listIssues("pied-piper"); });
    expectCode("missing_token", () => {
        listIssues("pied-piper", {
            limit: 100,
            cursor: "0:100:0",
            projects: ["1", "2"],
            environments: ["production", "canary/blue"],
            query: "",
            statsPeriod: "24h",
            groupStatsPeriod: "1h",
            sort: "freq",
            shortIdLookup: true,
        });
    });

    label("numeric and short issue IDs pass local validation");
    // Sentry counts short IDs in base32, so a real one is rarely all digits.
    for (const issueId of ["123456789", "BACKEND-123", "RUST-7E", "API-A", "MY-PROJECT-1F"]) {
        expectCode("missing_token", () => { getIssue("pied-piper", "backend", issueId); });
    }
    expectCode("missing_token", () => { updateIssue("pied-piper", "backend", "1", { clearAssignee: true }); });
    expectCode("missing_token", () => {
        updateIssue("pied-piper", "backend", "1", { status: "ignored", substatus: "archived_forever", assignedTo: "team:42", priority: "high" });
    });
}
