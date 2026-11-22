// Live reads against the account installed with `--config accountUrl=…` and the
// token bound as ZENDESK_ACCESS_TOKEN. Read-only: nothing here writes. Skipped by
// `--skip-network` and when the token is not bound. Steps that need data the
// account may not have (a ticket, a view, a macro, an article) say so and move on.
import { label } from "submilli:test";
import secrets from "submilli:secrets";
import {
    countTickets,
    countView,
    findOrganizations,
    findTickets,
    findUsers,
    getArticle,
    getOrganization,
    getTicket,
    getUser,
    listAgents,
    listBrands,
    listCategories,
    listComments,
    listGroups,
    listMacros,
    listSatisfactionRatings,
    listSections,
    listTicketFields,
    listViews,
    previewMacro,
    search,
    searchArticles,
    ticketMetrics,
    viewTickets,
    ZendeskError,
} from "@submilli/zendesk";

function main(): void {
    if (secrets.get("ZENDESK_ACCESS_TOKEN") === undefined) {
        label("skip: ZENDESK_ACCESS_TOKEN is not bound");
        return;
    }

    label("metadata: groups, agents, ticket fields, brands");
    const groups = listGroups();
    assert(groups.items.length > 0, "the account has a group");
    const agents = listAgents(groups.items[0].id);
    assert(agents.items.every((agent) => agent.role !== "end-user"), "a group's members are agents");
    const fields = listTicketFields();
    assert(fields.some((field) => field.type === "subject"), "the standard subject field");
    assert(listBrands().length > 0, "a brand");

    label("finding and counting tickets");
    const open = findTickets({ status: ["new", "open", "pending"] }, { pageSize: 5 });
    const count = countTickets({ status: ["new", "open", "pending"] });
    assert(open.count >= open.items.length && count >= 0, "counts");
    const sorted = findTickets({}, { sortBy: "created_at", sortOrder: "asc", pageSize: 2 });
    if (sorted.items.length === 2) assert(sorted.items[0].createdAt <= sorted.items[1].createdAt, "oldest first");
    if (sorted.nextCursor !== null) {
        const next = findTickets({}, { sortBy: "created_at", sortOrder: "asc", pageSize: 2, cursor: sorted.nextCursor });
        assert(next.items.length > 0 && next.items[0].id !== sorted.items[0].id, "the second page");
    }
    assert(getTicket("999999999") === null, "a missing ticket reads as null");

    const any = findTickets({}, { pageSize: 1 });
    if (any.items.length === 0) {
        label("skip: the account has no tickets");
    } else {
        label("one ticket: comments, metrics, requester, organization");
        const ticket = getTicket(any.items[0].id);
        assert(ticket !== null && ticket.id === any.items[0].id && ticket.status.length > 0, "read it back");
        const comments = listComments(any.items[0].id, { pageSize: 5 });
        assert(comments.items.length > 0, "a ticket has its first comment");
        const metrics = ticketMetrics(any.items[0].id);
        assert(metrics === null || metrics.ticketId === any.items[0].id, "metrics");
        const requester = any.items[0].requesterId;
        if (requester !== null) {
            const user = getUser(requester);
            assert(user !== null && user.id === requester, "the requester");
            const byRequester = findTickets({ requesterId: requester }, { pageSize: 5 });
            assert(byRequester.items.every((item) => item.requesterId === requester), "tickets by requester");
        }
        const organization = any.items[0].organizationId;
        if (organization !== null) assert(getOrganization(organization) !== null, "the organization");
    }

    label("people search");
    const users = findUsers("*", { pageSize: 5 });
    assert(users.items.length > 0, "users");
    const someone = users.items[0].name;
    if (someone.length >= 2) {
        const named = findUsers(someone, { pageSize: 5 });
        assert(named.items.some((user) => user.name === someone), "found by name");
    }
    findOrganizations("ex");

    label("views and macros");
    const views = listViews();
    if (views.items.length === 0) {
        label("skip: no active views");
    } else {
        const view = views.items[0];
        const tickets = viewTickets(view.id, { pageSize: 5 });
        assert(tickets.items.length <= 5, "a view's page");
        countView(view.id);
    }
    const macros = listMacros();
    if (macros.items.length === 0 || any.items.length === 0) {
        label("skip: no macro or no ticket to preview it on");
    } else {
        const preview = previewMacro(any.items[0].id, macros.items[0].id);
        assert(preview.comment === null || preview.comment.body.length > 0 || preview.comment.htmlBody.length > 0, "a macro preview");
    }

    label("Help Center");
    let hasGuide = true;
    try {
        listCategories();
    } catch (error: ZendeskError) {
        hasGuide = false;
        label("skip: no Help Center (" + error.errorCode + ")");
    }
    if (hasGuide) {
        const sections = listSections();
        const hits = searchArticles("the", { pageSize: 3 });
        if (hits.items.length === 0) {
            label("skip: no articles");
        } else {
            const article = getArticle(hits.items[0].id);
            assert(article !== null && article.title === hits.items[0].title, "read an article found");
            assert(sections.length > 0, "an article has a section");
        }
    }

    label("ratings and raw search");
    listSatisfactionRatings({ pageSize: 5 });
    const raw = search("status<solved", { type: "ticket", pageSize: 3 });
    assert(raw.items.every((item) => item["result_type"] === "ticket"), "raw results are tickets");
}
