// Run by `zendesk_ticket_rules_hold_against_a_fake_zendesk` in
// crates/submilli/tests/package_policy.rs. The package copy talks to a fake Zendesk, so each
// check runs with the group, status and role the package read from it, under the real policy
// engine. The test also asserts which writes reached the fake.
import { addNote, applyMacro, assignTicket, getTicket, getUser, reply, updateTicket, ZendeskError } from "@submilli/zendesk";

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
    // Reads: group 100's ticket; another group's is denied, and so is a missing one, so a
    // denial does not tell whether a ticket exists.
    const mine = getTicket("1");
    assert(mine !== null && mine.groupId === "100" && mine.status === "open", "read group 100's ticket");
    deniedAt("zendesk.com/tickets.get", "main", () => { getTicket("2"); });
    deniedAt("zendesk.com/tickets.get", "main", () => { getTicket("3"); });

    // Writes: the group comes from Zendesk, not from the caller.
    updateTicket("1", { status: "pending", addTags: ["vip"] });
    deniedAt("zendesk.com/tickets.update", "main", () => { updateTicket("1", { priority: "urgent" }); });
    deniedAt("zendesk.com/tickets.update", "main", () => { updateTicket("2", { status: "solved" }); });
    addNote("1", "Checked the logs");
    deniedAt("zendesk.com/comments.note", "main", () => { addNote("2", "Mine now"); });
    deniedAt("zendesk.com/comments.reply", "main", () => { reply("1", "We fixed it", { status: "solved" }); });
    assignTicket("1", { assigneeId: "20" });
    deniedAt("zendesk.com/tickets.assign", "main", () => { assignTicket("1", { groupId: "200" }); });

    // Macros: one that tags and notes is applied; one that replies publicly, or moves the
    // ticket to another group, is denied and writes nothing.
    applyMacro("1", "51");
    deniedAt("zendesk.com/comments.reply", "main", () => { applyMacro("1", "50"); });
    deniedAt("zendesk.com/macros.apply", "main", () => { applyMacro("1", "52"); });

    // A ticket changed since it was read is refused by Zendesk, not overwritten.
    let conflict = "";
    try {
        updateTicket("4", { status: "pending" });
    } catch (error: ZendeskError) {
        conflict = error.errorCode + " " + error.retryable.toString();
    }
    assert(conflict === "UpdateConflict true", "safe_update conflict, got " + conflict);

    // People: customers, not agents.
    assert(getUser("10") !== null, "a customer");
    deniedAt("zendesk.com/users.get", "main", () => { getUser("11"); });
    return "ticket rules hold against the fake Zendesk";
}
