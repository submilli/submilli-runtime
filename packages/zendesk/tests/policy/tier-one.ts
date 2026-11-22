// Run through `submilli run --blueprint tier-one.yaml`, not the allow-all build test runner.
// The checks here come before any request, so each allowed call stops when the package
// reads its token, which the blueprint does not grant it.
import {
    countTickets,
    countView,
    createTicket,
    createUser,
    findTickets,
    listBrands,
    listSatisfactionRatings,
    listTicketFields,
    search,
    searchArticles,
} from "@submilli/zendesk";

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
    deniedAt("secrets.get", "@submilli/zendesk", action);
}

function main(): string {
    // Its group's queue, by an id with a leading zero too; not everyone's, not closed tickets.
    allowed(() => { findTickets({ groupId: "100", status: ["open", "pending"] }); });
    allowed(() => { countTickets({ groupId: "0100" }); });
    deniedAt("zendesk.com/tickets.find", "main", () => { findTickets({ status: ["open"] }); });
    deniedAt("zendesk.com/tickets.find", "main", () => { findTickets({ groupId: "200" }); });
    deniedAt("zendesk.com/tickets.find", "main", () => { findTickets({ groupId: "100", status: ["closed"] }); });

    // Internal tickets in its group; a public first comment, another group or a guarded custom field is denied.
    allowed(() => { createTicket({ subject: "Follow up", body: "Call back", public: false, groupId: "100", priority: "high" }); });
    deniedAt("zendesk.com/tickets.create", "main", () => { createTicket({ subject: "Hi", body: "Hello", groupId: "100" }); });
    deniedAt("zendesk.com/tickets.create", "main", () => { createTicket({ subject: "Hi", body: "Hello", public: false, groupId: "200" }); });
    deniedAt("zendesk.com/tickets.create", "main", () => {
        createTicket({ subject: "Hi", body: "Hello", public: false, groupId: "100", customFields: [{ id: "360001", value: true }] });
    });

    // Customers without notes, outside any organization.
    allowed(() => { createUser({ name: "Dana Lee", email: "dana@example.com" }); });
    deniedAt("zendesk.com/users.create", "main", () => { createUser({ name: "Dana Lee", email: "dana@example.com", notes: "VIP" }); });
    deniedAt("zendesk.com/users.create", "main", () => { createUser({ name: "Dana Lee", email: "dana@example.com", organizationId: "5" }); });

    // English articles, the ticket fields, bad ratings and one view; no brands, no raw search.
    allowed(() => { searchArticles("refund", { locale: "EN-US" }); });
    deniedAt("zendesk.com/articles.search", "main", () => { searchArticles("refund"); });
    allowed(() => { listTicketFields(); });
    deniedAt("zendesk.com/metadata.read", "main", () => { listBrands(); });
    allowed(() => { listSatisfactionRatings({ score: "bad" }); });
    deniedAt("zendesk.com/ratings.list", "main", () => { listSatisfactionRatings(); });
    allowed(() => { countView("360000001"); });
    deniedAt("zendesk.com/views.read", "main", () => { countView("360000002"); });
    deniedAt("zendesk.com/search.raw", "main", () => { search("type:ticket status:open"); });
    return "the tier-one rules hold";
}
