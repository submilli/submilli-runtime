// Live writes against the account installed with `--config accountUrl=…`. Runs
// only with ZENDESK_ACCESS_TOKEN and ZENDESK_LIVE_MUTATIONS=true bound, with an
// admin's token. It creates a customer at example.com, tickets for that customer
// a macro and a Help Center category with a section and an article, all named
// with a run id, and deletes them at the end, also when a step fails. Deleting is not a package operation, so the cleanup calls the API
// directly. The reply goes to the test customer's example.com address.
import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { writeText } from "submilli:fs";
import { delete as httpDelete, get, post } from "submilli:http";
import { v4 } from "submilli:uuid";
import { accountUrl } from "../src/config";
import {
    addNote,
    applyMacro,
    assignTicket,
    countTickets,
    createTicket,
    createUser,
    findTickets,
    findUsers,
    getArticle,
    getTicket,
    listAgents,
    listComments,
    listGroups,
    listSections,
    mergeTickets,
    previewMacro,
    reply,
    updateTicket,
    updateUser,
    uploadAttachment,
} from "@submilli/zendesk";

const API = accountUrl + "/api/v2/";

function main(): void {
    if (secrets.get("ZENDESK_ACCESS_TOKEN") === undefined || secrets.get("ZENDESK_LIVE_MUTATIONS") !== "true") {
        label("skip: needs ZENDESK_ACCESS_TOKEN and ZENDESK_LIVE_MUTATIONS=true");
        return;
    }
    const run = v4().slice(0, 8);
    const tickets: string[] = [];
    const users: string[] = [];
    const macros: string[] = [];
    const categories: string[] = [];
    try {
        exercise(run, tickets, users, macros);
        helpCenter(run, categories);
    } finally {
        cleanUp(tickets, users, macros, categories);
    }
}

function exercise(run: string, tickets: string[], users: string[], macros: string[]): void {
    label("a customer");
    const customer = createUser({
        name: "Submilli Test " + run, email: "zd-test-" + run + "@example.com", tags: ["submilli-test"],
        notes: "Created by the package's live tests",
    });
    users.push(customer.id);
    assert(customer.role === "end-user", "an end user");
    const changed = updateUser(customer.id, { details: "Run " + run });
    assert(changed.details === "Run " + run, "details changed");

    label("an internal ticket for the customer");
    const created = createTicket({
        subject: "Submilli test " + run, body: "Opened by the live tests", public: false,
        requesterId: customer.id, priority: "low", type: "question", tags: ["submilli-test", run],
    });
    tickets.push(created.id);
    assert(created.requesterId === customer.id && created.priority === "low", "created as asked");

    label("status, priority and tags; tags added and removed");
    const updated = updateTicket(created.id, { status: "open", priority: "high", addTags: ["vip"], removeTags: [run] });
    assert(updated.status === "open" && updated.priority === "high", "status and priority");
    assert(updated.tags.includes("vip") && !updated.tags.includes(run), "tags " + JSON.stringify(updated.tags));

    label("a note with an attachment, then a public reply");
    writeText("/tmp-note.txt", "log line " + run + "\n");
    const upload = uploadAttachment("/tmp-note.txt", { contentType: "text/plain" });
    assert(upload.token.length > 0 && upload.attachment.fileName === "tmp-note.txt", "uploaded");
    addNote(created.id, "Checked the logs for " + run, { attachments: [upload.token] });
    reply(created.id, "Thanks for writing in. (" + run + ")", { status: "pending" });
    const conversation = listComments(created.id);
    const count = conversation.items.length;
    assert(count === 3, "three comments, got " + count.toString());
    assert(!conversation.items[1].public && conversation.items[1].attachments.length === 1, "the note is private, with its file");
    assert(conversation.items[2].public, "the reply is public");
    const afterReply = getTicket(created.id);
    assert(afterReply !== null && afterReply.status === "pending", "the reply set the status");

    label("assignment to a group and an agent");
    const groups = listGroups();
    const group = groups.items[0].id;
    const agents = listAgents(group);
    const assigned = assignTicket(created.id, { groupId: group, assigneeId: agents.items.length > 0 ? agents.items[0].id : null });
    assert(assigned.groupId === group, "moved to the group");
    const unassigned = assignTicket(created.id, { assigneeId: null });
    assert(unassigned.assigneeId === null && unassigned.groupId === group, "unassigned, still in the group");

    label("a macro previewed, then applied");
    const macro = body(post(API + "macros.json", { macro: { title: "Submilli test " + run, actions: [
        { field: "status", value: "open" },
        { field: "current_tags", value: "macro-" + run },
        { field: "comment_value", value: "Macro note " + run },
        { field: "comment_mode_is_public", value: "false" },
    ] } }, headers()));
    const macroId = idOf(macro["macro"]);
    macros.push(macroId);
    const preview = previewMacro(created.id, macroId);
    assert(preview.comment !== null && !preview.comment.public, "the macro adds a note");
    const applied = applyMacro(created.id, macroId);
    assert(applied.status === "open" && applied.tags.includes("macro-" + run), "applied " + JSON.stringify(applied.tags));

    label("finding the run's tickets and the customer");
    const byRequester = findTickets({ requesterId: customer.id });
    assert(byRequester.items.length <= 1, "search may lag; at most the one ticket");
    countTickets({ requesterId: customer.id, tag: "submilli-test" });
    findUsers("zd-test-" + run + "@example.com");

    label("a duplicate merged into the first");
    const duplicate = createTicket({ subject: "Submilli test duplicate " + run, body: "Same issue", public: false, requesterId: customer.id });
    tickets.push(duplicate.id);
    const job = mergeTickets(created.id, [duplicate.id], { targetComment: "Merged a duplicate", sourceComment: "Merged into " + created.id });
    assert(job.id.length > 0, "a merge job " + job.status);
}

// An article in a new section of a new category, written through the API (the
// package only reads the Help Center), then read back through the package.
function helpCenter(run: string, categories: string[]): void {
    label("a Help Center article");
    const category = body(post(API + "help_center/categories.json", { category: { name: "Submilli test " + run, locale: "en-us" } }, headers()));
    const categoryId = idOf(category["category"]);
    categories.push(categoryId);
    const section = body(post(API + "help_center/categories/" + categoryId + "/sections.json", { section: { name: "Submilli test " + run, locale: "en-us" } }, headers()));
    const sectionId = idOf(section["section"]);
    const groups = body(get(API + "guide/permission_groups.json", headers()));
    const permissionGroup = idOf((groups["permission_groups"] as unknown[])[0]);
    const created = body(post(API + "help_center/sections/" + sectionId + "/articles.json", { article: {
        title: "Submilli test article " + run, body: "<p>Resetting a password, run " + run + ".</p>", locale: "en-us",
        permission_group_id: Number(permissionGroup), user_segment_id: null,
    } }, headers()));
    const articleId = idOf(created["article"]);
    const article = getArticle(articleId);
    assert(article !== null && article.sectionId === sectionId && article.body.includes(run), "read the article back");
    assert(listSections({ categoryId: categoryId }).some((one) => one.id === sectionId), "the category's section");
}

// Deletes what the run created: tickets, then the customer, the macro and the Help
// Center category (with its section and article). Each failure is reported, not
// thrown, so the rest still goes.
function cleanUp(tickets: string[], users: string[], macros: string[], categories: string[]): void {
    label("clean up");
    // One at a time: `destroy_many` is a background job, and a customer cannot be
    // deleted while their tickets still exist.
    for (const ticket of tickets) report("ticket " + ticket, httpDelete(API + "tickets/" + ticket + ".json", headers()).status);
    for (const user of users) report("user " + user, httpDelete(API + "users/" + user + ".json", headers()).status);
    for (const macro of macros) report("macro " + macro, httpDelete(API + "macros/" + macro + ".json", headers()).status);
    for (const category of categories) report("category " + category, httpDelete(API + "help_center/categories/" + category + ".json", headers()).status);
}

function report(what: string, status: number): void {
    if (status >= 300) label("clean up failed for " + what + ": HTTP " + status.toString());
}

function headers(): Map<string, string> {
    const map = new Map<string, string>();
    map.set("Authorization", "Bearer " + (secrets.get("ZENDESK_ACCESS_TOKEN") ?? ""));
    map.set("Accept", "application/json");
    return map;
}

function body(response: { ok: boolean; status: number; body: string }): Record<string, unknown> {
    assert(response.ok, "HTTP " + response.status.toString() + ": " + response.body);
    return JSON.parse(response.body) as Record<string, unknown>;
}

function idOf(raw: unknown): string {
    const id = (raw as Record<string, unknown>)["id"];
    return typeof id === "number" ? id.toString() : "";
}
