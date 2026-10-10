import { label } from "submilli:test";
import {
    SentryError,
    buildIssueQuery,
    listOrganizations,
    buildUpdateIssueBody,
    isShortIssueId,
    normalizeEventJson,
    parseNextCursor,
    sentryErrorCode,
    sentryFailureMessage,
} from "@submilli/sentry";

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
    label("issue query keeps Sentry defaults and validates page size");
    assert(buildIssueQuery() === "?limit=50", "default page size only");
    assert(buildIssueQuery({}) === "?limit=50", "empty options match omitted options");
    assert(buildIssueQuery({ limit: 1, cursor: "0:100:0" }) === "?limit=1&cursor=0%3A100%3A0", "cursor encoded");
    expectCode("invalid_page_size", () => { buildIssueQuery({ limit: 0 }); });
    expectCode("invalid_page_size", () => { buildIssueQuery({ limit: 4.5 }); });

    label("issue query preserves repeated filters and explicit empty query");
    const query = buildIssueQuery({
        limit: 100,
        projects: ["1", "2"],
        environments: ["production", "canary/blue"],
        query: "",
        statsPeriod: "24h",
        groupStatsPeriod: "1h",
        sort: "freq",
        shortIdLookup: true,
    });
    assert(
        query === "?limit=100&project=1&project=2&environment=production&environment=canary%2Fblue&query=&statsPeriod=24h&groupStatsPeriod=1h&sort=freq&shortIdLookup=1",
        "repeated parameters retain order",
    );
    expectCode("invalid_time_range", () => {
        buildIssueQuery({ statsPeriod: "24h", start: "2026-01-01T00:00:00Z", end: "2026-01-02T00:00:00Z" });
    });
    expectCode("invalid_time_range", () => { buildIssueQuery({ start: "2026-01-01T00:00:00Z" }); });

    label("short ID detection distinguishes opaque Sentry group IDs");
    assert(!isShortIssueId("123456789"), "numeric issue ID is direct");
    assert(isShortIssueId("BACKEND-123"), "human short ID resolves");
    // Sentry counts short IDs in base32, so a real one is rarely all digits.
    // `RUST-7E` is what the live API hands back for the third issue in a project.
    assert(isShortIssueId("RUST-7E"), "base32 short-ID suffix resolves");
    assert(isShortIssueId("API-A"), "single letter suffix resolves");
    assert(isShortIssueId("MY-PROJECT-1F"), "hyphenated project slug keeps its short ID");
    assert(!isShortIssueId("not-an-id"), "malformed ref is not mistaken for a valid short ID");
    assert(!isShortIssueId("BACKEND-"), "empty suffix is not a short ID");

    label("Link pagination honors the results flag");
    const next = "<https://sentry.io/api/0/organizations/acme/issues/?cursor=0%3A100%3A0>; rel=\"next\"; results=\"true\"; cursor=\"0:100:0\"";
    assert(parseNextCursor(next) === "0:100:0", "next cursor parsed from metadata");
    const last = "<https://sentry.io/api/0/organizations/acme/issues/?cursor=0%3A100%3A0>; rel=\"next\"; results=\"false\"; cursor=\"0:100:0\"";
    assert(parseNextCursor(last) === "", "false results marks final page");
    assert(parseNextCursor("") === "", "missing header marks final page");

    label("triage bodies are partial and explicit");
    assert(buildUpdateIssueBody({ status: "resolved" }) === "{\"status\":\"resolved\"}", "status only");
    assert(buildUpdateIssueBody({ clearAssignee: true }) === "{\"assignedTo\":null}", "assignment clears with null");
    assert(
        buildUpdateIssueBody({ status: "ignored", substatus: "archived_forever", assignedTo: "team:42", priority: "high" }) ===
            "{\"status\":\"ignored\",\"substatus\":\"archived_forever\",\"assignedTo\":\"team:42\",\"priority\":\"high\"}",
        "combined core triage update",
    );
    expectCode("empty_update", () => { buildUpdateIssueBody({}); });
    expectCode("invalid_update", () => { buildUpdateIssueBody({ assignedTo: "user:1", clearAssignee: true }); });
    expectCode("invalid_update", () => { buildUpdateIssueBody({ substatus: "archived_forever" }); });
    expectCode("invalid_update", () => { buildUpdateIssueBody({ status: "unresolved", substatus: "archived_forever" }); });

    label("event JSON normalizes supported entries and ignores unknown entries");
    const event = normalizeEventJson(
        "{\"id\":\"e1\",\"eventID\":\"abc\",\"groupID\":\"g1\",\"projectID\":\"p1\",\"title\":\"TypeError\",\"dateCreated\":\"2026-07-27T10:00:00Z\",\"tags\":[{\"key\":\"environment\",\"value\":\"production\"}],\"entries\":[{\"type\":\"exception\",\"data\":{\"values\":[{\"type\":\"TypeError\",\"value\":\"boom\",\"threadId\":7,\"mechanism\":{\"type\":\"generic\",\"handled\":false},\"stacktrace\":{\"frames\":[{\"filename\":\"app.ts\",\"function\":\"main\",\"lineNo\":10,\"context\":[[9,\"before\"],[10,\"throw boom\"]]}]}}]}},{\"type\":\"breadcrumbs\",\"data\":{\"values\":[{\"category\":\"http\",\"message\":\"GET /api\",\"data\":{\"url\":\"https://example.com/api\",\"method\":\"GET\",\"status_code\":500}}]}},{\"type\":\"request\",\"data\":{\"url\":\"https://example.com/api\",\"method\":\"GET\",\"headers\":[[\"accept\",\"application/json\"]],\"query\":[[\"debug\",\"1\"]]}},{\"type\":\"unknown-future-entry\",\"data\":{\"ignored\":true}}]}"
    );
    assert(event.summary.eventId === "abc", "event ID normalized");
    assert(event.summary.tags.length === 1, "tags normalized");
    assert(event.exceptions.length === 1, "exception normalized");
    assert(event.exceptions[0].threadId === "7", "numeric thread ID normalized");
    assert(event.exceptions[0].frames[0].context.length === 2, "source context normalized");
    assert(event.breadcrumbs[0].statusCode === "500", "breadcrumb status normalized");
    assert(event.request !== null, "request normalized");
    if (event.request !== null) assert(event.request.headers[0].key === "accept", "request headers normalized");

    label("minimal event JSON receives stable empty defaults");
    const minimal = normalizeEventJson("{}");
    assert(minimal.summary.id === "", "missing scalar defaults empty");
    assert(minimal.exceptions.length === 0, "missing entries default empty");
    assert(minimal.request === null, "missing request defaults null");

    label("JSON nulls read like missing fields");
    const nulls = normalizeEventJson(
        "{\"id\":\"e2\",\"eventID\":null,\"tags\":null,\"user\":null,\"release\":null,\"entries\":[{\"type\":\"request\",\"data\":null},{\"type\":\"exception\",\"data\":{\"values\":[{\"stacktrace\":null,\"mechanism\":null,\"threadId\":null}]}}]}"
    );
    assert(nulls.summary.eventId === "e2", "a null event ID falls back to the row ID");
    assert(nulls.summary.tags.length === 0, "null tags default empty");
    assert(nulls.user === null && nulls.release === null, "null contexts stay null");
    assert(nulls.request === null, "a request entry without data is skipped");
    assert(nulls.exceptions[0].frames.length === 0 && nulls.exceptions[0].mechanism === null, "null stacktrace and mechanism default");

    label("HTTP errors expose stable codes and actionable messages");
    assert(sentryErrorCode(401) === "unauthorized", "401 mapping");
    assert(sentryErrorCode(429) === "rate_limited", "429 mapping");
    assert(sentryErrorCode(503) === "http_error", "fallback mapping");
    assert(sentryFailureMessage(400, "Bad Request", "{\"detail\":\"Invalid query\"}") === "Invalid query", "detail lifted");
    assert(
        sentryFailureMessage(400, "Bad Request", "{\"detail\":\"\",\"error\":null,\"message\":\"Bad cursor\"}") === "Bad cursor",
        "empty and null fields fall through to the next one",
    );
    assert(
        sentryFailureMessage(502, "Bad Gateway", "<html>upstream</html>") === "Sentry request failed: HTTP 502 Bad Gateway",
        "non-JSON body falls back",
    );
    assert(
        sentryFailureMessage(502, "Bad Gateway", "{malformed") === "Sentry request failed: HTTP 502 Bad Gateway",
        "malformed JSON body falls back",
    );

    label("an unbound auth token is reported before any request");
    expectCode("missing_token", () => { listOrganizations(); });
}
