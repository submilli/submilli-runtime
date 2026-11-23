// Offline tests. The validators and mappers live in the package's internal
// modules, so these tests import them from `../src/`. The operations section
// checks that each operation validates its input before any request: invalid
// input fails with a local ZendeskError (status 0), and valid input stops when
// the token is read (`missing_token`). That section is skipped when the secret
// is bound, so this file never sends a request.
import { label } from "submilli:test";
import secrets from "submilli:secrets";
import {
    addNote,
    applyMacro,
    assignTicket,
    countTickets,
    createTicket,
    createUser,
    findOrganizations,
    findTickets,
    findUsers,
    getArticle,
    getTicket,
    getUser,
    listComments,
    listSatisfactionRatings,
    listViews,
    mergeTickets,
    previewMacro,
    reply,
    search,
    searchArticles,
    updateTicket,
    updateUser,
    uploadAttachment,
    viewTickets,
    ZendeskError,
} from "@submilli/zendesk";
import {
    customValue,
    isCursor,
    normalizeId,
    normalizeTag,
    pageQuery,
    pageWindow,
    requireDate,
    searchPhrase,
    searchWindow,
    ticketStatus,
} from "../src/request";
import {
    afterCursor,
    commentFrom,
    failureFrom,
    macroPreviewFrom,
    metricsFrom,
    retryAfterSeconds,
    ticketFrom,
    userFrom,
} from "../src/response";

function codeOf(action: () => void): string {
    try {
        action();
    } catch (error: ZendeskError) {
        return error.status === 0 ? error.errorCode : "HTTP " + error.status.toString();
    } catch (error) {
        return error.name + ": " + error.message;
    }
    return "no error";
}

function expectCode(expected: string, action: () => void, what: string): void {
    const code = codeOf(action);
    assert(code === expected, what + ": expected " + expected + ", got " + code);
}

function main(): void {
    label("ids are decimal digits, a safe integer");
    assert(normalizeId(" 35436 ", "id") === "35436", "trimmed");
    assert(normalizeId("007", "id") === "7", "leading zeros dropped");
    for (const bad of ["", "0", "-1", "1.5", "12a", "1e5", "../1", "99999999999999999"]) {
        expectCode("invalid_id", () => { normalizeId(bad, "id"); }, "refused " + JSON.stringify(bad));
    }

    label("statuses, tags, dates and custom values");
    assert(ticketStatus(" Pending ", "status") === "pending", "a status in any case");
    expectCode("invalid_input", () => { ticketStatus("waiting", "status"); }, "an unknown status");
    assert(normalizeTag(" VIP ", "tags") === "vip", "a tag lowercased");
    for (const bad of ["", "two words", "a\"b", "a,b", "a\nb"]) {
        expectCode("invalid_tag", () => { normalizeTag(bad, "tags"); }, "refused tag " + JSON.stringify(bad));
    }
    assert(requireDate("2026-11-01", "d") === "2026-11-01", "a date");
    assert(requireDate("2026-11-01T09:00:00Z", "d") === "2026-11-01T09:00:00Z", "a date-time");
    for (const bad of ["11/01/2026", "2026-11-01 09:00", "2026-11-01T09:00 OR x", "tomorrow"]) {
        expectCode("invalid_date", () => { requireDate(bad, "d"); }, "refused date " + JSON.stringify(bad));
    }
    assert(JSON.stringify(customValue(["Gold", "silver"], "v")) === "[\"gold\",\"silver\"]", "multi-select tags");
    expectCode("invalid_input", () => { customValue(1 / 0, "v"); }, "infinity");

    label("search phrases cannot break out of their quotes");
    assert(searchPhrase(" refund please ", "text") === "\"refund please\"", "quoted");
    expectCode("invalid_input", () => { searchPhrase("x\" status:closed \"y", "text"); }, "a quote");
    expectCode("invalid_input", () => { searchPhrase(" ", "text"); }, "blank");

    label("pages");
    assert(pageQuery(pageWindow({})) === "page%5Bsize%5D=100", "the first page");
    assert(pageQuery(pageWindow({ pageSize: 10, cursor: "eyJvIjoibmljZV9pZCJ9" })) === "page%5Bsize%5D=10&page%5Bafter%5D=eyJvIjoibmljZV9pZCJ9", "a later page");
    expectCode("invalid_input", () => { pageWindow({ pageSize: 101 }); }, "too large");
    expectCode("invalid_cursor", () => { pageWindow({ cursor: "a&b=c" }); }, "a cursor with a query");
    assert(isCursor("aGVsbG8=") && !isCursor(""), "cursor shape");
    assert(searchWindow({ pageSize: 100, cursor: "10" }).page === 10, "the tenth search page");
    expectCode("invalid_cursor", () => { searchWindow({ pageSize: 100, cursor: "11" }); }, "past 1,000 results");
    expectCode("invalid_cursor", () => { searchWindow({ cursor: "1" }); }, "page one is not a cursor");
    assert(afterCursor({ meta: { has_more: true, after_cursor: "abc" } }) === "abc", "more");
    assert(afterCursor({ meta: { has_more: false, after_cursor: "abc" } }) === null, "the last page");

    label("responses");
    const ticket = ticketFrom({
        id: 35436, subject: "Help", description: "It broke", status: "open", priority: null, type: "incident", tags: ["vip"],
        requester_id: 20978392, submitter_id: 20978392, assignee_id: null, group_id: 98738, organization_id: 509974, brand_id: 1,
        follower_ids: [5], email_cc_ids: [], via: { channel: "email" },
        custom_fields: [{ id: 27642, value: "745" }, { id: 27648, value: ["a", "b"] }, { id: 1, value: null }],
        created_at: "2026-11-01T09:00:00Z", updated_at: "2026-11-02T09:00:00Z", due_at: null,
    });
    assert(ticket.id === "35436" && ticket.groupId === "98738" && ticket.assigneeId === null, "ids as text");
    assert(ticket.priority === null && ticket.type === "incident" && ticket.channel === "email", "fields");
    assert(ticket.customFields.length === 3 && JSON.stringify(ticket.customFields[1].value) === "[\"a\",\"b\"]", "custom fields");
    const comment = commentFrom({ id: 1, author_id: 2, body: "Hi **there**", plain_body: "Hi there", html_body: "<p>Hi</p>", public: false,
        attachments: [{ id: 9, file_name: "a.png", content_type: "image/png", size: 10, content_url: "https://x/a.png" }], via: { channel: "web" } });
    assert(comment.body === "Hi there" && !comment.public && comment.attachments[0].fileName === "a.png", "a comment");
    const user = userFrom({ id: 1, name: "Dana", role: "end-user", organization_id: 5, user_fields: { plan: "gold", seats: 3, other: { x: 1 } } });
    assert(user.role === "end-user" && user.organizationId === "5" && user.userFields["plan"] === "gold" && user.userFields["other"] === null, "a user");
    const metrics = metricsFrom({ replies: 2, reply_time_in_minutes: { calendar: 10, business: 5 }, solved_at: null }, "7");
    assert(metrics.firstReplyMinutes.business === 5 && metrics.fullResolutionMinutes.calendar === null, "metrics");
    const preview = macroPreviewFrom({ result: { ticket: { status: "solved", comment: { body: "Thanks", public: true } } } });
    assert(preview.changes["status"] === "solved" && preview.comment !== null && preview.comment.public, "a macro preview");
    assert(macroPreviewFrom({ result: { ticket: { status: "open" } } }).comment === null, "a macro without a comment");
    const internal = macroPreviewFrom({ result: { ticket: { comment: { body: "Checked", public: "false" } } } }).comment;
    assert(internal !== null && !internal.public, "Zendesk sends a macro comment's visibility as text");
    const visible = macroPreviewFrom({ result: { ticket: { comment: { body: "Thanks", public: "true" } } } }).comment;
    assert(visible !== null && visible.public, "and \"true\" is public");

    label("errors");
    const invalid = failureFrom(422, "Unprocessable", "{\"error\":\"RecordInvalid\",\"description\":\"Record validation errors\",\"details\":{\"requester\":[{\"description\":\"Requester: is invalid\"}]}}");
    assert(invalid.errorCode === "RecordInvalid" && invalid.details[0] === "requester: Requester: is invalid" && !invalid.retryable, "RecordInvalid");
    const forbidden = failureFrom(403, "Forbidden", "{\"error\":{\"title\":\"Forbidden\",\"message\":\"You do not have access\"}}");
    assert(forbidden.errorCode === "Forbidden" && forbidden.message.includes("You do not have access"), "nested error");
    assert(failureFrom(401, "Unauthorized", "{\"error\":\"invalid_token\",\"error_description\":\"expired\"}").errorCode === "invalid_token", "OAuth error");
    const conflict = failureFrom(409, "Conflict", "{\"error\":\"UpdateConflict\"}");
    assert(conflict.errorCode === "UpdateConflict" && conflict.retryable, "a safe_update conflict");
    assert(failureFrom(429, "Too Many Requests", "").retryable && failureFrom(500, "Error", "<html>").errorCode === "ServerError", "no body");
    assert(retryAfterSeconds("30") === 30 && retryAfterSeconds("Wed, 21 Oct") === null && retryAfterSeconds("") === null, "Retry-After");

    if (secrets.get("ZENDESK_ACCESS_TOKEN") !== undefined) return;

    label("operations validate their input before any request");
    expectCode("invalid_id", () => { getTicket("ticket-1"); }, "a ticket id");
    expectCode("invalid_input", () => { findTickets({ text: "a\" OR \"b" }); }, "a phrase with quotes");
    expectCode("invalid_date", () => { findTickets({ createdAfter: "last week" }); }, "a date filter");
    expectCode("invalid_tag", () => { findTickets({ tag: "two words" }); }, "a tag filter");
    expectCode("invalid_id", () => { countTickets({ groupId: "Billing" }); }, "a group by name");
    expectCode("invalid_input", () => { createTicket({ subject: " ", body: "x" }); }, "a blank subject");
    expectCode("invalid_input", () => { createTicket({ subject: "x", body: "y", customFields: [{ id: "1", value: 1 }, { id: "01", value: 2 }] }); }, "a repeated custom field");
    expectCode("invalid_input", () => { updateTicket("1", {}); }, "no changes");
    expectCode("invalid_input", () => { updateTicket("1", { tags: ["a"], addTags: ["b"] }); }, "tags and addTags");
    expectCode("invalid_input", () => { assignTicket("1", {}); }, "an empty assignment");
    expectCode("invalid_id", () => { assignTicket("1", { assigneeId: "dana" }); }, "an assignee by name");
    expectCode("invalid_input", () => { mergeTickets("1", []); }, "no sources");
    expectCode("invalid_input", () => { mergeTickets("1", ["1"]); }, "the target as a source");
    expectCode("invalid_input", () => { mergeTickets("1", ["2", "3", "4", "5", "6", "7"]); }, "too many sources");
    expectCode("invalid_input", () => { addNote("1", " "); }, "a blank note");
    expectCode("invalid_input", () => { listComments("1", { pageSize: 0 }); }, "a page size");
    expectCode("invalid_input", () => { uploadAttachment("/dir/"); }, "an upload path");
    expectCode("invalid_id", () => { getUser("me"); }, "a user id");
    expectCode("invalid_input", () => { findUsers(" "); }, "a blank user query");
    expectCode("invalid_input", () => { createUser({ name: "Dana", userFields: [{ key: "plan", value: 1 }, { key: "plan", value: 2 }] }); }, "a repeated user field");
    expectCode("invalid_input", () => { updateUser("1", {}); }, "no user changes");
    expectCode("invalid_input", () => { findOrganizations("a"); }, "a one-letter organization");
    expectCode("invalid_cursor", () => { listViews({ cursor: "x y" }); }, "a views cursor");
    expectCode("invalid_id", () => { viewTickets("unassigned"); }, "a view id");
    expectCode("invalid_id", () => { previewMacro("1", "close"); }, "a macro id");
    expectCode("invalid_id", () => { applyMacro("x", "1"); }, "a ticket id for a macro");
    expectCode("invalid_input", () => { searchArticles("refund", { locale: "en_US;" }); }, "a locale");
    expectCode("invalid_input", () => { getArticle("1", { locale: "../x" }); }, "an article locale");

    label("valid input stops at the missing credential");
    for (const action of [
        () => { getTicket("1"); },
        () => { findTickets({ status: ["open"], groupId: "2", text: "refund" }); },
        () => { updateTicket("1", { status: "pending", addTags: ["vip"] }); },
        () => { assignTicket("1", { groupId: "2", assigneeId: null }); },
        () => { reply("1", "Thanks", { status: "solved" }); },
        () => { addNote("1", "Checked the logs"); },
        () => { createTicket({ subject: "Help", body: "It broke", public: false }); },
        () => { searchArticles("refund") ; },
    ]) {
        const code = codeOf(action);
        assert(code === "missing_token", "stopped at the credential, got " + code);
    }
}
