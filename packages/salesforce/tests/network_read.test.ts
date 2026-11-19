// Live reads against the org installed with `--config orgUrl=…` and the token
// bound as SALESFORCE_ACCESS_TOKEN. Read-only: nothing here writes. Skipped by
// `--skip-network` and when the token is not bound. Steps that need data the
// org may not have (an account, a report, a dashboard) say so and move on.
import { label } from "submilli:test";
import secrets from "submilli:secrets";
import {
    aggregate,
    describeObject,
    describeReport,
    findAccounts,
    findCases,
    findContacts,
    findLeads,
    findOpportunities,
    findPricebooks,
    findUsers,
    getAccount,
    getContact,
    getCurrentUser,
    getDashboard,
    getLimits,
    getPricebook,
    getRecord,
    getRelated,
    getUser,
    listActivities,
    listDashboards,
    listListViews,
    listObjects,
    listPendingApprovals,
    listReports,
    query,
    readFeed,
    runListView,
    runReport,
    SalesforceError,
    search,
    soql,
    sosl,
} from "@submilli/salesforce";

const ABSENT_ACCOUNT = "001000000000000AAA";
// Characters SOSL reserves in a FIND clause.
const RESERVED = "?&|!{}[]()^~*:\\\"'+-";

function main(): void {
    if (secrets.get("SALESFORCE_ACCESS_TOKEN") === undefined) {
        label("skip: SALESFORCE_ACCESS_TOKEN is not bound");
        return;
    }

    label("the token's user, limits and objects");
    const me = getCurrentUser();
    assert(me.id.length === 18 && me.name.length > 0, "the current user has an id and a name");
    const user = getUser(me.id);
    assert(user !== null && user.id === me.id, "getUser reads the current user");
    assert(findUsers({}, { pageSize: 200 }).records.length > 0, "findUsers lists users");
    let hasApiRequests = false;
    for (const limit of getLimits()) {
        if (limit.name === "DailyApiRequests") hasApiRequests = limit.max > 0 && limit.remaining <= limit.max;
    }
    assert(hasApiRequests, "DailyApiRequests is among the limits");
    let hasAccount = false;
    for (const object of listObjects()) {
        if (object.name === "Account") hasAccount = object.queryable && object.keyPrefix === "001";
    }
    assert(hasAccount, "Account is listed, queryable, with prefix 001");

    label("describeObject");
    const account = describeObject("Account");
    assert(account !== null, "Account is described");
    let nameField = false;
    let contacts = false;
    for (const field of account!.fields) {
        if (field.name === "Name") nameField = field.type === "string" && !field.nillable;
    }
    for (const child of account!.childRelationships) {
        if (child.relationshipName === "Contacts") contacts = child.childObject === "Contact" && child.field === "AccountId";
    }
    assert(nameField, "Account.Name is a required string");
    assert(contacts, "Account has the Contacts relationship");
    const opportunity = describeObject("Opportunity");
    let stagePicklist = false;
    for (const field of opportunity!.fields) {
        if (field.name === "StageName") stagePicklist = field.type === "picklist" && field.picklistValues.length > 0;
    }
    assert(stagePicklist, "Opportunity.StageName is a picklist with values");
    assert(describeObject("No_Such_Object__c") === null, "an unknown object describes as null");

    label("typed reads and an absent record");
    assert(getAccount(ABSENT_ACCOUNT) === null, "an absent account reads as null");
    assert(getRecord("Account", ABSENT_ACCOUNT) === null, "an absent record reads as null");
    const accounts = findAccounts({}, { pageSize: 200 });
    assert(accounts.records.length <= accounts.totalSize, "a page holds at most the total");
    if (accounts.records.length === 0) {
        label("skip: the org has no accounts");
    } else {
        const first = accounts.records[0];
        const again = getAccount(first.id, { fields: ["Owner.Email"] });
        assert(again !== null && again.name === first.name, "getAccount reads the listed account");
        assert(again!.createdDate !== null && again!.lastModifiedDate !== null, "system dates are read");
        assert(Object.keys(again!.custom).indexOf("Owner.Email") >= 0, "extra fields are returned in custom");
        const raw = getRecord("Account", first.id, { fields: ["Name", "OwnerId", "BillingAddress"] });
        assert(raw !== null && raw.object === "Account" && raw.fields["Name"] === first.name, "getRecord reads by id");

        const related = getRelated("Account", first.id, "Contacts", { fields: ["Id", "Name", "AccountId"] });
        for (const contact of related.records) {
            assert(contact.fields["AccountId"] === first.id, "related contacts belong to the account");
        }
        const typed = findContacts({ accountId: first.id });
        assert(typed.totalSize === related.totalSize, "findContacts and getRelated agree");
        if (typed.records.length > 0) {
            const contact = getContact(typed.records[0].id);
            assert(contact !== null && contact.accountId === first.id && contact.name === typed.records[0].name, "getContact reads a listed contact");
        }
        const cases = findCases({ accountId: first.id }, { pageSize: 200 });
        for (const kase of cases.records) assert(kase.accountId === first.id && kase.caseNumber !== null, "findCases keeps to the account");

        label("activities and feed of an account");
        for (const activity of listActivities(first.id, { limit: 20 })) {
            assert(activity.id.length === 18 && activity.whatId !== null, "activity history entries have ids");
        }
        for (const activity of listActivities(first.id, { open: true, limit: 20 })) {
            assert(!activity.isClosed, "open activities are open");
        }
        const feed = readFeed(first.id, { pageSize: 5 });
        assert(feed.posts.length <= 5, "a feed page holds at most its size");

        label("search finds the account by name");
        const name = first.name === null ? "" : first.name;
        if (name.length >= 2) {
            const found = search({ text: name, objects: [{ object: "Account", fields: ["Name"] }], limit: 20 });
            let hit = false;
            for (const record of found) {
                if (record.id === first.id) hit = true;
            }
            assert(hit, "search finds the account (" + name + ")");
            let escaped = "";
            for (const c of name) escaped += (RESERVED.indexOf(c) >= 0 ? "\\" : "") + c;
            const raw = sosl("FIND {" + escaped + "} IN NAME FIELDS RETURNING Account(Id, Name)");
            let rawHit = false;
            for (const record of raw) {
                if (record.id === first.id) rawHit = record.object === "Account";
            }
            assert(rawHit, "raw SOSL finds the account");
        }
    }

    label("paging with a cursor");
    const paged = query({ object: "Account", fields: ["Name"], orderBy: [{ field: "Name" }] }, { pageSize: 200 });
    if (paged.nextCursor !== null) {
        const next = query({ object: "Account", fields: ["Name"], orderBy: [{ field: "Name" }] }, { pageSize: 200, cursor: paged.nextCursor });
        assert(next.records.length > 0 && next.records[0].id !== paged.records[0].id, "the next page holds other records");
    } else {
        assert(paged.done && paged.records.length === paged.totalSize, "one page holds everything");
    }

    label("aggregates, raw SOQL and opportunities");
    const byStage = aggregate({ object: "Opportunity", groupBy: [{ field: "StageName" }], measures: [{ fn: "count" }, { fn: "sum", field: "Amount" }] });
    assert(byStage.groups[0] === "StageName" && byStage.measures[0] === "count()" && byStage.measures[1] === "sum(Amount)", "labels are as given");
    let counted = 0;
    for (const row of byStage.rows) counted = counted + (row.measures["count()"] as number);
    const total = aggregate({ object: "Opportunity", measures: [{ fn: "count" }] });
    assert(total.rows.length === 1 && total.rows[0].measures["count()"] === counted, "group counts add up to the total");
    const byMonth = aggregate({ object: "Opportunity", groupBy: [{ field: "CloseDate", date: "calendar_month" }], measures: [{ fn: "count" }] });
    assert(byMonth.groups[0] === "calendar_month(CloseDate)", "date groups are labelled by function and field");
    const open = findOpportunities({ isClosed: false }, { pageSize: 200 });
    for (const deal of open.records) assert(deal.stage !== null, "open opportunities have a stage");
    const text = soql("SELECT Id, Name FROM Account ORDER BY Name LIMIT 5");
    assert(text.records.length <= 5, "raw SOQL keeps its limit");

    label("leads and price books");
    for (const lead of findLeads({ isConverted: false }, { pageSize: 200 }).records) assert(!lead.isConverted, "unconverted leads are unconverted");
    const standard = findPricebooks({ isStandard: true });
    assert(standard.records.length === 1, "the org has one standard price book");
    const pricebook = getPricebook(standard.records[0].id);
    assert(pricebook !== null && pricebook.isStandard, "getPricebook reads it");

    label("list views");
    const views = listListViews("Account");
    assert(views.length > 0, "Account has list views");
    const view = runListView("Account", views[0].id, { pageSize: 200 });
    assert(view.records.length <= view.totalSize, "a list view runs");

    label("reports and dashboards");
    const reports = listReports({ pageSize: 200 });
    if (reports.records.length === 0) {
        label("skip: the org has no reports");
    } else {
        let ran = false;
        let refused = 0;
        for (const summary of reports.records) {
            if (ran) continue;
            try {
                const described = describeReport(summary.id);
                assert(described !== null && described.id === summary.id, "describeReport reads " + summary.id);
                const result = runReport(summary.id, { raw: true });
                assert(result.reportId === summary.id && result.raw !== null, "runReport returns the raw result too");
                assert(result.totals.length > 0, "a report has at least its grand total");
                const grand = result.totals[result.totals.length - 1];
                assert(Object.keys(grand.groups).length === 0, "the grand total is last");
                for (const row of result.rows) {
                    for (const column of result.columns) {
                        assert(Object.keys(row).indexOf(column.name) >= 0, "each row has every column: " + column.name);
                    }
                }
                const totalsOnly = runReport(summary.id, { includeDetails: false });
                assert(totalsOnly.rows.length === 0, "totals only has no rows");
                ran = true;
            } catch (error) {
                // Some reports cannot be read through the API: joined reports, and
                // reports on features the org has no licence for (403).
                if (!(error instanceof SalesforceError)) throw error;
                refused = refused + 1;
            }
        }
        if (refused > 0) label(refused.toString() + " reports were refused before one ran");
        assert(ran, "at least one report runs");
    }
    const dashboards = listDashboards({ pageSize: 200 });
    if (dashboards.records.length === 0) {
        label("skip: the org has no dashboards");
    } else {
        const dashboard = getDashboard(dashboards.records[0].id);
        assert(dashboard !== null && dashboard.components.length >= 0, "getDashboard reads the first dashboard");
    }

    label("pending approvals");
    const pending = listPendingApprovals({ pageSize: 200 });
    assert(pending.records.length <= pending.totalSize, "pending approvals list");
}

main();
