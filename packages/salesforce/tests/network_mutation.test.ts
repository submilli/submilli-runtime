// Live writes against the org installed with `--config orgUrl=…`. Runs only
// with SALESFORCE_ACCESS_TOKEN and SALESFORCE_LIVE_MUTATIONS=true bound. Every
// record it creates is named with a run id and deleted at the end, also when a
// step fails; Chatter posts and the uploaded file go with their account.
// Upserts, approvals and the flow need the test metadata in `tests/org/`
// deployed and are skipped without it. An email is sent only to
// SALESFORCE_LIVE_EMAIL_TO, when it is bound.
import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { readText, writeText } from "submilli:fs";
import { v4 } from "submilli:uuid";
import {
    approve,
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
    describeObject,
    downloadFile,
    exportQuery,
    findCampaignMembers,
    findCampaigns,
    findCases,
    findContacts,
    findContracts,
    findEvents,
    findLeads,
    findOpportunityLineItems,
    findOrders,
    findPricebooks,
    findProducts,
    findTasks,
    getAccount,
    getCampaign,
    getCampaignMember,
    getCase,
    getContact,
    getContract,
    getCurrentUser,
    getDeleted,
    getEvent,
    getLead,
    getOpportunity,
    getOpportunityLineItem,
    getOrder,
    getPricebook,
    getProduct,
    getRecord,
    getTask,
    getUpdated,
    listActivities,
    listPendingApprovals,
    logActivity,
    postToFeed,
    readFeed,
    reject,
    runFlow,
    sendEmail,
    submitForApproval,
    updateCase,
    updateLead,
    updateOpportunity,
    updateRecord,
    updateRecords,
    uploadFile,
    upsertRecord,
} from "@submilli/salesforce";

// Records to delete at the end, children before the accounts they belong to.
const created: { object: string; id: string }[] = [];

// What `tests/org/` deploys.
const EXTERNAL_ID = "Submilli_Test_Key__c";
const APPROVAL_PROCESS = "Submilli_Test_Approval";
const FLOW = "Submilli_Test_Flow";

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
    const contact = getContact(contactId);
    assert(contact !== null && contact.lastName === "Tester" && contact.accountId === accountId && contact.email === "live.tester@example.com", "getContact reads the contact");

    label("cases");
    const caseId = track("Case", createCase({ subject: run, accountId: accountId, contactId: contactId, priority: "Low" }));
    updateCase(caseId, { priority: "High", description: "escalated" });
    const kase = getCase(caseId);
    assert(kase !== null && kase.priority === "High" && kase.accountId === accountId && kase.caseNumber !== null, "the case was created and updated");
    const cases = findCases({ accountId: accountId, priority: "High" });
    assert(cases.totalSize === 1 && cases.records[0].id === caseId, "findCases finds the case");

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
    const task = getTask(taskId);
    assert(task !== null && task.whatId === accountId && task.whoId === contactId && task.isClosed, "getTask reads the logged call");
    const tasks = findTasks({ whatId: accountId, isClosed: true });
    assert(tasks.totalSize === 1 && tasks.records[0].id === taskId, "findTasks finds the call");
    const events = findEvents({ whatId: accountId, startAfter: "2030-01-01T00:00:00Z" });
    assert(events.totalSize === 1 && events.records[0].id === eventId, "findEvents finds the event");

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
    const leads = findLeads({ company: run + " Lead Co", isConverted: false });
    assert(leads.totalSize === 1 && leads.records[0].id === leadId, "findLeads finds the lead");
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

    label("products, price books and line items");
    const standard = findPricebooks({ isStandard: true });
    assert(standard.records.length === 1, "the org has one standard price book");
    const pricebookId = standard.records[0].id;
    const pricebook = getPricebook(pricebookId);
    assert(pricebook !== null && pricebook.isStandard, "getPricebook reads it");
    const productId = track("Product2", createRecord("Product2", [{ field: "Name", value: run + " product" }, { field: "ProductCode", value: "SUB-TEST" }, { field: "IsActive", value: true }]));
    const products = findProducts({ name: run, isActive: true });
    assert(products.totalSize === 1 && products.records[0].id === productId, "findProducts finds the product");
    const product = getProduct(productId);
    assert(product !== null && product.productCode === "SUB-TEST" && product.isActive, "getProduct reads it");
    const entryId = track("PricebookEntry", createRecord("PricebookEntry", [
        { field: "Pricebook2Id", value: pricebookId }, { field: "Product2Id", value: productId }, { field: "UnitPrice", value: 100 }, { field: "IsActive", value: true },
    ]));
    const lineItemId = track("OpportunityLineItem", createRecord("OpportunityLineItem", [
        { field: "OpportunityId", value: converted.opportunityId! }, { field: "PricebookEntryId", value: entryId }, { field: "Quantity", value: 2 }, { field: "UnitPrice", value: 100 },
    ]));
    const lineItems = findOpportunityLineItems({ opportunityId: converted.opportunityId! });
    assert(lineItems.totalSize === 1 && lineItems.records[0].id === lineItemId, "findOpportunityLineItems finds the line item");
    const lineItem = getOpportunityLineItem(lineItemId);
    assert(lineItem !== null && lineItem.product2Id === productId && lineItem.productName === run + " product" && lineItem.totalPrice === 200, "getOpportunityLineItem reads it");

    label("campaigns");
    const campaignId = track("Campaign", createRecord("Campaign", [{ field: "Name", value: run + " campaign" }, { field: "IsActive", value: true }, { field: "Status", value: "Planned" }]));
    const campaigns = findCampaigns({ name: run, isActive: true });
    assert(campaigns.totalSize === 1 && campaigns.records[0].id === campaignId, "findCampaigns finds the campaign");
    const campaign = getCampaign(campaignId);
    assert(campaign !== null && campaign.status === "Planned" && campaign.ownerId === me.id, "getCampaign reads it");
    const memberId = track("CampaignMember", createRecord("CampaignMember", [{ field: "CampaignId", value: campaignId }, { field: "ContactId", value: contactId }]));
    const members = findCampaignMembers({ campaignId: campaignId });
    assert(members.totalSize === 1 && members.records[0].id === memberId, "findCampaignMembers finds the member");
    const member = getCampaignMember(memberId);
    assert(member !== null && member.contactId === contactId && member.campaignName === run + " campaign", "getCampaignMember reads it");

    label("contracts and orders");
    const contractId = track("Contract", createRecord("Contract", [
        { field: "AccountId", value: accountId }, { field: "StartDate", value: day }, { field: "ContractTerm", value: 12 }, { field: "Status", value: "Draft" },
    ]));
    const contracts = findContracts({ accountId: accountId });
    assert(contracts.totalSize === 1 && contracts.records[0].id === contractId, "findContracts finds the contract");
    const contract = getContract(contractId);
    assert(contract !== null && contract.contractTerm === 12 && contract.contractNumber !== null && contract.accountName === run, "getContract reads it");
    const orderId = track("Order", createRecord("Order", [
        { field: "AccountId", value: accountId }, { field: "EffectiveDate", value: day }, { field: "Status", value: "Draft" },
    ]));
    const orders = findOrders({ accountId: accountId, status: "Draft" });
    assert(orders.totalSize === 1 && orders.records[0].id === orderId, "findOrders finds the order");
    const order = getOrder(orderId);
    assert(order !== null && order.orderNumber !== null && order.effectiveDate === day, "getOrder reads it");

    let deployed = false;
    for (const field of describeObject("Account")!.fields) {
        if (field.name === EXTERNAL_ID) deployed = true;
    }
    if (!deployed) {
        label("skip: upserts, approvals and the flow need tests/org deployed");
    } else {
        label("upsert by external id");
        const key = run.substring(run.length - 8);
        const inserted = upsertRecord("Account", EXTERNAL_ID, key, [{ field: "Name", value: run + " upserted" }]);
        assert(inserted.created, "the first upsert creates");
        track("Account", inserted.id);
        const again = upsertRecord("Account", EXTERNAL_ID, key, [{ field: "Phone", value: "555-0199" }]);
        assert(!again.created && again.id === inserted.id, "the second upsert updates the same account");
        const upserted = getRecord("Account", inserted.id, { fields: ["Name", "Phone"] });
        assert(upserted !== null && upserted.fields["Phone"] === "555-0199" && upserted.fields["Name"] === run + " upserted", "the update kept the name");

        label("approvals");
        const approved = track("Account", createRecord("Account", [{ field: "Name", value: run + " to approve" }]));
        const submitted = submitForApproval(approved, { processName: APPROVAL_PROCESS, nextApproverIds: [me.id], comment: "please" });
        assert(submitted.status === "Pending" && submitted.recordId === approved && submitted.workItemIds.length === 1, "the submission waits for one approver");
        assert(submitted.actorIds.indexOf(me.id) >= 0, "the chosen approver is asked");
        let waiting = false;
        for (const item of listPendingApprovals({ actorId: me.id }).records) {
            if (item.workItemId === submitted.workItemIds[0]) waiting = item.recordId === approved && item.processName === APPROVAL_PROCESS && item.object === "Account";
        }
        assert(waiting, "the approval is pending for the approver");
        const decided = approve(submitted.workItemIds[0], { comment: "fine" });
        assert(decided.status === "Approved" && decided.workItemIds.length === 0, "approve approves");
        const rejected = track("Account", createRecord("Account", [{ field: "Name", value: run + " to reject" }]));
        const second = submitForApproval(rejected, { processName: APPROVAL_PROCESS, nextApproverIds: [me.id] });
        assert(reject(second.workItemIds[0], { comment: "no" }).status === "Rejected", "reject rejects");

        label("a flow");
        const flow = runFlow(FLOW, [{ name: "name", value: "Ada" }, { name: "count", value: 21 }]);
        assert(flow.outputs["greeting"] === "Hello Ada" && flow.outputs["doubled"] === 42, "the flow's outputs come back");
    }

    const to = secrets.get("SALESFORCE_LIVE_EMAIL_TO");
    if (to === undefined) {
        label("skip: SALESFORCE_LIVE_EMAIL_TO is not bound");
    } else {
        label("an email");
        sendEmail({ to: [to], subject: run + " email", body: "Sent by the @submilli/salesforce live test.", relatedTo: accountId });
    }

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
        // The account, A, B and the converted lead's; and with tests/org, the upserted and the two submitted.
        const accounts = deployed ? 7 : 4;
        assert(exported.records === accounts, "the export holds the run's " + accounts.toString() + " accounts, got " + exported.records.toString());
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
