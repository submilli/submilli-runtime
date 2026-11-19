// Live writes against the org installed with `--config orgUrl=…`. Runs only
// with SALESFORCE_ACCESS_TOKEN and SALESFORCE_LIVE_MUTATIONS=true bound. Every
// record it creates is named with a run id and deleted at the end, also when a
// step fails; Chatter posts and the uploaded file go with their account. It
// sends no email, submits no approval and runs no flow.
import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { readText, writeText } from "submilli:fs";
import { v4 } from "submilli:uuid";
import {
    changeOwner,
    convertLead,
    createCase,
    createContact,
    createEvent,
    createLead,
    createNote,
    createRecord,
    createRecords,
    deleteRecord,
    downloadFile,
    exportQuery,
    findContacts,
    getAccount,
    getCase,
    getCurrentUser,
    getDeleted,
    getEvent,
    getLead,
    getOpportunity,
    getRecord,
    getUpdated,
    listActivities,
    logActivity,
    postToFeed,
    readFeed,
    updateCase,
    updateLead,
    updateOpportunity,
    updateRecord,
    updateRecords,
    uploadFile,
} from "@submilli/salesforce";

// Records to delete at the end, children before the accounts they belong to.
const created: { object: string; id: string }[] = [];

function track(object: string, id: string): string {
    created.push({ object: object, id: id });
    return id;
}

function cleanUp(): void {
    for (let i = created.length - 1; i >= 0; i = i - 1) {
        try {
            deleteRecord(created[i].object, created[i].id);
        } catch (error) {
            label("could not delete " + created[i].object + " " + created[i].id + ": " + error.message);
        }
    }
}

function main(): void {
    if (secrets.get("SALESFORCE_ACCESS_TOKEN") === undefined || secrets.get("SALESFORCE_LIVE_MUTATIONS") !== "true") {
        label("skip: Salesforce live mutations are disabled");
        return;
    }
    const run = "Submilli test " + v4().substring(0, 8);
    try {
        writes(run);
    } finally {
        cleanUp();
    }
}

function writes(run: string): void {
    const me = getCurrentUser();

    label("createRecord, updateRecord and the typed account");
    const accountId = track("Account", createRecord("Account", [{ field: "Name", value: run }, { field: "Industry", value: "Technology" }]));
    updateRecord("Account", accountId, [{ field: "Description", value: "changed" }, { field: "Industry", value: null }]);
    const account = getAccount(accountId);
    assert(account !== null && account.name === run && account.description === "changed", "the account was created and updated");
    assert(account!.industry === null, "null clears a field");
    assert(account!.ownerId === me.id, "the token's user owns what it creates");
    const createdAt = account!.createdDate!;

    label("batch writes");
    const batch = createRecords("Account", [[{ field: "Name", value: run + " A" }], [{ field: "Name", value: run + " B" }]]);
    assert(batch.length === 2 && batch[0].success && batch[1].success, "both accounts were created");
    track("Account", batch[0].id!);
    track("Account", batch[1].id!);
    const updated = updateRecords("Account", [
        { id: batch[0].id!, values: [{ field: "Phone", value: "555-0100" }] },
        { id: batch[1].id!, values: [{ field: "Phone", value: "555-0101" }] },
    ]);
    assert(updated.length === 2 && updated[0].success && updated[1].success, "both accounts were updated");
    const b = getRecord("Account", batch[1].id!, { fields: ["Phone"] });
    assert(b !== null && b.fields["Phone"] === "555-0101", "the batch update reached the second account");

    label("contacts");
    const contactId = track("Contact", createContact({ lastName: "Tester", firstName: "Live", accountId: accountId, email: "live.tester@example.com" }));
    const contacts = findContacts({ accountId: accountId });
    assert(contacts.totalSize === 1 && contacts.records[0].id === contactId, "the contact belongs to the account");

    label("cases");
    const caseId = track("Case", createCase({ subject: run, accountId: accountId, contactId: contactId, priority: "Low" }));
    updateCase(caseId, { priority: "High", description: "escalated" });
    const kase = getCase(caseId);
    assert(kase !== null && kase.priority === "High" && kase.accountId === accountId && kase.caseNumber !== null, "the case was created and updated");

    label("activities");
    const day = createdAt.substring(0, 10);
    const taskId = track("Task", logActivity({ type: "call", subject: run + " call", whatId: accountId, whoId: contactId, date: day, description: "Talked" }));
    const eventId = track("Event", createEvent({ subject: run + " meeting", start: "2030-01-15T15:00:00Z", end: "2030-01-15T15:30:00Z", whatId: accountId, location: "Video" }));
    const event = getEvent(eventId);
    assert(event !== null && event.location === "Video" && event.whatId === accountId, "the event was created");
    let logged = false;
    for (const activity of listActivities(accountId)) {
        if (activity.id === taskId) logged = activity.isTask && activity.isClosed && activity.subtype === "Call";
    }
    assert(logged, "the logged call is in the account's activity history");
    let upcoming = false;
    for (const activity of listActivities(accountId, { open: true })) {
        if (activity.id === eventId) upcoming = !activity.isTask && activity.start !== null;
    }
    assert(upcoming, "the event is among the account's open activities");

    label("notes, files and the feed");
    track("ContentNote", createNote(accountId, run + " note", "First line\nSecond line with <b>markup</b> & more"));
    writeText("/salesforce-upload.txt", "uploaded by " + run);
    const file = uploadFile(accountId, "/salesforce-upload.txt", { title: run + " file" });
    track("ContentDocument", file.contentDocumentId);
    const downloaded = downloadFile(file.contentDocumentId, "/salesforce-download.txt", { overwrite: true });
    assert(downloaded.title === run + " file" && downloaded.bytes > 0, "the file downloads with its title");
    assert(readText("/salesforce-download.txt") === "uploaded by " + run, "the downloaded bytes are the uploaded ones");
    const postId = postToFeed(accountId, run + " posted", { mentions: [me.id] });
    let posted = false;
    for (const post of readFeed(accountId).posts) {
        if (post.id === postId) posted = post.text.indexOf(run + " posted") >= 0 && post.authorId === me.id;
    }
    assert(posted, "the post is in the account's feed");

    label("leads and conversion");
    const leadId = track("Lead", createLead({ lastName: "Prospect", firstName: "Live", company: run + " Lead Co", email: "live.prospect@example.com" }));
    updateLead(leadId, { title: "CTO", rating: "Hot" });
    const lead = getLead(leadId);
    assert(lead !== null && lead.title === "CTO" && lead.rating === "Hot" && !lead.isConverted, "the lead was created and updated");
    const converted = convertLead(leadId, { opportunityName: run + " deal" });
    track("Account", converted.accountId);
    track("Contact", converted.contactId);
    assert(converted.opportunityId !== null, "conversion created an opportunity");
    track("Opportunity", converted.opportunityId!);
    const after = getLead(leadId);
    assert(after !== null && after.isConverted && after.convertedOpportunityId === converted.opportunityId, "the lead reads as converted");

    label("opportunities and owners");
    updateOpportunity(converted.opportunityId!, { stage: "Negotiation/Review", amount: 4200, nextStep: "Send terms" });
    const deal = getOpportunity(converted.opportunityId!);
    assert(deal !== null && deal.stage === "Negotiation/Review" && deal.amount === 4200 && deal.name === run + " deal", "the opportunity was updated");
    changeOwner("Opportunity", converted.opportunityId!, me.id);

    label("changes over a time window");
    const start = day + "T00:00:00Z";
    const end = day + "T23:59:00Z";
    const changed = getUpdated("Account", start, end);
    assert(changed.ids.indexOf(accountId) >= 0, "the account is among today's changes");

    label("a bulk export");
    let exported = exportQuery({ object: "Account", fields: ["Name"], where: [{ field: "Name", op: "like", value: run + "%" }] }, "/salesforce-export.csv");
    for (let i = 0; i < 60 && !exported.done; i = i + 1) {
        exported = exportQuery({ object: "Account", fields: ["Name"], where: [{ field: "Name", op: "like", value: run + "%" }] }, "/salesforce-export.csv", { jobId: exported.jobId });
    }
    if (exported.done) {
        assert(exported.records === 4, "the export holds the run's four accounts, got " + exported.records.toString());
        assert(readText("/salesforce-export.csv").indexOf(run + " A") >= 0, "the CSV names them");
    } else {
        label("the export was still " + exported.state + "; not waited for");
    }

    label("deletes");
    assert(deleteRecord("Account", batch[1].id!), "deleteRecord reports a deletion");
    assert(!deleteRecord("Account", batch[1].id!), "deleting it again reports none");
    const gone = getDeleted("Account", start, end);
    let listed = false;
    for (const record of gone.records) {
        if (record.id === batch[1].id!) listed = true;
    }
    assert(listed, "the deleted account is among today's deletions");
}

main();
