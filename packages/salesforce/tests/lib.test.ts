// Offline tests. The builders, normalizers and mappers live in the package's
// internal modules, so these tests import them from `../src/`. The operations
// section checks that each operation validates its input before any request:
// invalid input fails with a local SalesforceError (status 0), and valid input
// stops when the org's URL or the token is read (`missing_…`). That section is
// skipped when the secrets are bound, so this file never sends a request.
import { label } from "submilli:test";
import secrets from "submilli:secrets";
import {
    aggregate,
    approve,
    changeOwner,
    convertLead,
    createCase,
    createEvent,
    createNote,
    describeReport,
    downloadFile,
    listActivities,
    listPendingApprovals,
    logActivity,
    postToFeed,
    readFeed,
    runFlow,
    runListView,
    runReport,
    sendEmail,
    submitForApproval,
    uploadFile,
    createContact,
    createRecord,
    createRecords,
    exportQuery,
    findOpportunities,
    getAccount,
    getRecord,
    query,
    SalesforceError,
    search,
    updateOpportunity,
    updateRecord,
    updateRecords,
    upsertRecord,
} from "@submilli/salesforce";
import {
    idAmong,
    isCursor,
    normalizeId,
    normalizeName,
    normalizePath,
    pageWindow,
    prepareValues,
    refuseActivityLinks,
    refuseScopeChange,
} from "../src/request";
import {
    buildAggregate,
    buildQuery,
    buildSearch,
    containsLiteral,
    escapeSosl,
    prepareCondition,
    quote,
    relativeDate,
} from "../src/soql";
import { extraFields, opportunityFrom, typedSoql, Writes } from "../src/objects";
import { aggregateFrom, batchFrom, cursorFrom, failureFrom, fieldsOf, recordFrom } from "../src/response";
import {
    activitiesFrom,
    taskSubtype,
    activitySoql,
    emailAddress,
    escapeHtml,
    feedBody,
    feedCursor,
    feedPageFrom,
    recipients,
} from "../src/activity";
import { approvalFrom, convertedFrom, convertEnvelope, flowFrom, pendingFrom, xmlValue } from "../src/process";
import { dashboardFrom, reportDescriptionFrom, reportFilter, reportFrom, withFilters } from "../src/reports";
import { limitsFrom, objectDescriptionFrom } from "../src/metadata";

function codeOf(action: () => void): string {
    try {
        action();
    } catch (error: SalesforceError) {
        return error.status === 0 ? error.errorCode : "HTTP " + error.status.toString();
    } catch (error) {
        return error.name + ": " + error.message;
    }
    return "no error";
}

function main(): void {
    label("record ids normalize to their 18-character form");
    assert(normalizeId("001D000000IqhSL", "id") === "001D000000IqhSLIAZ", "15 gains its checksum");
    assert(normalizeId("001D000000IqhSLIAZ", "id") === "001D000000IqhSLIAZ", "18 unchanged");
    assert(normalizeId("001d000000iqhslIAZ", "id") === "001D000000IqhSLIAZ", "case restored from the checksum");
    assert(normalizeId("001D000000IqhSLiaz", "id") === "001D000000IqhSLIAZ", "checksum in any case");
    assert(codeOf(() => { normalizeId("001D000000IqhS", "id"); }) === "invalid_id", "14 characters");
    assert(codeOf(() => { normalizeId("001D000000IqhSL'", "id"); }) === "invalid_id", "a quote");
    assert(codeOf(() => { normalizeId("001D00000/IqhSL", "id"); }) === "invalid_id", "a slash");
    assert(codeOf(() => { normalizeId("001D000000IqhSL999", "id"); }) === "invalid_id", "a checksum that names a digit as upper case");

    label("names and paths");
    assert(normalizeName(" Region__c ", "f").name === "Region__c", "trimmed");
    assert(normalizeName("Region__c", "f").key === "region__c", "lowercased key");
    assert(normalizePath("Account.Owner.Name", "f").key === "account.owner.name", "a path");
    for (const bad of ["", "1Name", "Name_", "Na me", "Name,Id", "Name)", "Account..Name", "A.B.C.D.E.F.G"]) {
        assert(codeOf(() => { normalizePath(bad, "f"); }) === "invalid_name", "refused " + JSON.stringify(bad));
    }

    label("values: names checked, Id and duplicates refused");
    const values = prepareValues([{ field: "StageName", value: "Closed Won" }, { field: "Amount", value: 1200 }, { field: "Region__c", value: null }], "values");
    assert(JSON.stringify(values.keys) === JSON.stringify(["amount", "region__c", "stagename"]), "sorted keys " + JSON.stringify(values.keys));
    assert(JSON.stringify(values.body) === JSON.stringify({ StageName: "Closed Won", Amount: 1200, Region__c: null }), "body");
    assert(prepareValues([{ field: "OwnerId", value: "005000000000001" }], "values").keys[0] === "ownerid", "owner allowed on create");
    label("updates may not move a record out of its owner's or account's scope");
    assert(codeOf(() => { refuseScopeChange(["ownerid"], "opportunity", "values"); }) === "owner_field", "owner refused");
    assert(codeOf(() => { refuseScopeChange(["accountid", "name"], "contact", "values"); }) === "account_field", "account refused");
    assert(codeOf(() => { refuseScopeChange(["whatid"], "task", "values"); }) === "account_field", "a task's record refused");
    refuseScopeChange(["whatid", "name"], "custom__c", "values");
    assert(codeOf(() => { refuseActivityLinks(["whoid"], "event", "values"); }) === "account_field", "a new event's contact refused");
    refuseActivityLinks(["accountid"], "contact", "values");
    assert(codeOf(() => { prepareValues([{ field: "Id", value: "x" }], "values"); }) === "invalid_input", "Id refused");
    assert(codeOf(() => { prepareValues([{ field: "Name", value: "a" }, { field: "NAME", value: "b" }], "values"); }) === "duplicate_field", "duplicate in another case");
    assert(codeOf(() => { prepareValues([{ field: "Amount", value: 1 / 0 }], "values"); }) === "invalid_input", "infinity");
    assert(idAmong([{ field: "accountid", value: "001D000000IqhSL" }], "accountid", "values") === "001D000000IqhSLIAZ", "an id among values");
    assert(idAmong([{ field: "Name", value: "x" }], "accountid", "values") === "", "absent");

    label("typed writes refuse a custom field a typed argument owns, given or not");
    const writes = new Writes();
    writes.set("StageName", "Prospecting");
    writes.setId("AccountId", undefined, "accountId");
    assert(codeOf(() => { writes.prepare([{ field: "stagename", value: "Closed Won" }], "changes"); }) === "typed_field", "a given typed field");
    assert(codeOf(() => { writes.prepare([{ field: "AccountId", value: "001000000000001" }], "changes"); }) === "typed_field", "a typed field not given");
    assert(codeOf(() => { writes.prepare([{ field: "OwnerId", value: "005000000000001" }], "changes"); }) === "owner_field", "owner in custom");
    assert(codeOf(() => { new Writes().prepare([], "changes"); }) === "invalid_input", "nothing to change");

    label("SOQL literals escape quotes, backslashes and control characters");
    assert(quote("O'Brien") === "'O\\'Brien'", "quote");
    assert(quote("a\\b") === "'a\\\\b'", "backslash");
    assert(quote("x' OR Name != '") === "'x\\' OR Name != \\''", "injection stays inside the literal");
    assert(quote("line\nbreak") === "'line\\nbreak'", "newline");
    assert(containsLiteral("50%_off") === "'%50\\%\\_off%'", "wildcards matched literally");
    assert(escapeSosl("acme (inc) - a&b") === "acme \\(inc\\) \\- a\\&b", "SOSL reserved characters");

    label("conditions");
    assert(prepareCondition({ field: "StageName", op: "=", value: "Closed Won" }).soql === "StageName = 'Closed Won'", "text");
    assert(prepareCondition({ field: "Amount", op: ">=", value: 1000 }).soql === "Amount >= 1000", "number");
    assert(prepareCondition({ field: "IsClosed", op: "=", value: false }).soql === "IsClosed = FALSE", "checkbox");
    assert(prepareCondition({ field: "CloseDate", op: "=", value: null }).soql === "CloseDate = NULL", "null");
    assert(prepareCondition({ field: "StageName", op: "in", values: ["A", "B'"] }).soql === "StageName IN ('A', 'B\\'')", "in");
    assert(prepareCondition({ field: "Tags__c", op: "includes", values: ["x"] }).soql === "Tags__c INCLUDES ('x')", "includes");
    assert(prepareCondition({ field: "CloseDate", op: ">", date: "2026-11-01" }).soql === "CloseDate > 2026-11-01", "date");
    assert(prepareCondition({ field: "CreatedDate", op: ">", dateTime: "2026-11-01T09:30:00Z" }).soql === "CreatedDate > 2026-11-01T09:30:00Z", "date-time");
    assert(prepareCondition({ field: "CloseDate", op: "=", dateLiteral: "this_quarter" }).soql === "CloseDate = THIS_QUARTER", "date literal");
    assert(prepareCondition({ field: "Name", op: "like", value: "Acme%" }).soql === "Name LIKE 'Acme%'", "like");
    assert(prepareCondition({ field: "Account.Industry", op: "=", value: "Tech" }).key === "account.industry", "path key");
    assert(relativeDate("LAST_N_DAYS:30", "f") === "LAST_N_DAYS:30", "n days");
    const refusals: (() => void)[] = [
        () => { prepareCondition({ field: "Name", op: "=" }); },
        () => { prepareCondition({ field: "Name", op: "=", value: "a", date: "2026-01-01" }); },
        () => { prepareCondition({ field: "Name", op: "in", value: "a" }); },
        () => { prepareCondition({ field: "Name", op: "=", values: ["a"] }); },
        () => { prepareCondition({ field: "Name", op: "in", values: [] }); },
        () => { prepareCondition({ field: "Amount", op: "<", value: null }); },
        () => { prepareCondition({ field: "IsWon", op: ">", value: true }); },
        () => { prepareCondition({ field: "Amount", op: "like", value: 3 }); },
        () => { prepareCondition({ field: "Amount", op: "=", value: 1e21 }); },
        () => { prepareCondition({ field: "CloseDate", op: "=", date: "2026-13-01" }); },
        () => { prepareCondition({ field: "CloseDate", op: "=", dateTime: "2026-11-01T09:30:00" }); },
        () => { prepareCondition({ field: "CloseDate", op: "=", dateLiteral: "TODAY OR Id != null" }); },
        () => { prepareCondition({ field: "Name = 'x' OR Name", op: "=", value: "y" }); },
    ];
    for (const refusal of refusals) {
        const code = codeOf(refusal);
        assert(code === "invalid_condition" || code === "invalid_date" || code === "invalid_name", "refused, got " + code);
    }

    label("queries");
    const built = buildQuery({
        object: "Opportunity",
        fields: ["Name", "Amount", "Account.Name", "name"],
        where: [{ field: "StageName", op: "!=", value: "Closed Lost" }, { field: "OwnerId", op: "=", value: "005000000000001AAA" }],
        orderBy: [{ field: "Amount", direction: "desc", nulls: "last" }],
        limit: 50,
    });
    assert(
        built.soql === "SELECT Id, Name, Amount, Account.Name FROM Opportunity WHERE StageName != 'Closed Lost' AND OwnerId = '005000000000001AAA' ORDER BY Amount DESC NULLS LAST LIMIT 50",
        built.soql,
    );
    assert(built.object === "opportunity", "object key");
    assert(JSON.stringify(built.fields) === JSON.stringify(["account.name", "amount", "name", "ownerid", "stagename"]), JSON.stringify(built.fields));
    assert(codeOf(() => { buildQuery({ object: "Opportunity", fields: [] }); }) === "invalid_input", "no fields");
    assert(codeOf(() => { buildQuery({ object: "Opportunity", fields: ["Name"], limit: 0 }); }) === "invalid_input", "limit 0");
    assert(codeOf(() => { buildQuery({ object: "Opportunity WHERE", fields: ["Name"] }); }) === "invalid_name", "object name");

    label("aggregates");
    const pipeline = buildAggregate({
        object: "Opportunity",
        groupBy: [{ field: "StageName" }, { field: "CloseDate", date: "calendar_month" }],
        measures: [{ fn: "count" }, { fn: "sum", field: "Amount" }],
        where: [{ field: "IsClosed", op: "=", value: false }],
        orderBy: [{ by: "sum(Amount)", direction: "desc" }],
    });
    assert(
        pipeline.soql === "SELECT StageName g0, CALENDAR_MONTH(CloseDate) g1, COUNT(Id) m0, SUM(Amount) m1 FROM Opportunity WHERE IsClosed = FALSE GROUP BY StageName, CALENDAR_MONTH(CloseDate) ORDER BY SUM(Amount) DESC LIMIT 2000",
        pipeline.soql,
    );
    assert(JSON.stringify(pipeline.groups) === JSON.stringify(["StageName", "calendar_month(CloseDate)"]), "group labels");
    assert(JSON.stringify(pipeline.measures) === JSON.stringify(["count()", "sum(Amount)"]), "measure labels");
    assert(JSON.stringify(pipeline.fields) === JSON.stringify(["amount", "closedate", "isclosed", "stagename"]), JSON.stringify(pipeline.fields));
    const overall = buildAggregate({ object: "Opportunity", measures: [{ fn: "count" }] });
    assert(overall.soql === "SELECT COUNT(Id) m0 FROM Opportunity", "an ungrouped aggregate has no LIMIT: " + overall.soql);
    assert(codeOf(() => { buildAggregate({ object: "Opportunity", measures: [{ fn: "sum" }] }); }) === "invalid_input", "sum needs a field");
    assert(codeOf(() => { buildAggregate({ object: "Opportunity", measures: [{ fn: "count" }], orderBy: [{ by: "Amount" }] }); }) === "invalid_input", "unknown label");
    const rows = aggregateFrom(
        { totalSize: 1, done: true, records: [{ attributes: { type: "AggregateResult" }, g0: "Prospecting", g1: 11, m0: 3, m1: null }] },
        pipeline.groups, pipeline.measures,
    );
    assert(rows.rows[0].groups["calendar_month(CloseDate)"] === 11, "group value");
    assert(rows.rows[0].measures["count()"] === 3 && rows.rows[0].measures["sum(Amount)"] === null, "measures");

    label("search");
    const found = buildSearch({ text: "acme (inc)", objects: [{ object: "Account", fields: ["Name"], limit: 5 }, { object: "Contact" }] });
    assert(
        JSON.stringify(found.body) === JSON.stringify({
            q: "acme \\(inc\\)", in: "ALL",
            sobjects: [{ name: "Account", fields: ["Id", "Name"], limit: 5 }, { name: "Contact", fields: ["Id"] }],
            overallLimit: 200,
        }),
        JSON.stringify(found.body),
    );
    assert(JSON.stringify(found.objects) === JSON.stringify(["account", "contact"]), "objects");
    assert(codeOf(() => { buildSearch({ text: "a", objects: [{ object: "Account" }] }); }) === "invalid_input", "short text");

    label("typed reads select the standard fields, relations and custom fields");
    const extra = extraFields(["Region__c", "Account.Industry", "Description"]);
    assert(
        typedSoql("Opportunity", extra, [], "LastModifiedDate DESC") === "SELECT FIELDS(STANDARD), Account.Name, Owner.Name, Region__c, Account.Industry FROM Opportunity ORDER BY LastModifiedDate DESC",
        typedSoql("Opportunity", extra, [], "LastModifiedDate DESC"),
    );
    const opportunity = opportunityFrom(
        fieldsOf({
            attributes: { type: "Opportunity" }, Id: "006000000000001AAA", Name: "Acme", StageName: "Prospecting", Amount: 1200,
            IsClosed: false, Account: { attributes: { type: "Account" }, Name: "Acme Inc", Industry: "Tech" }, REGION__C: "EMEA",
            Description: null,
        }),
        extra.requested,
    );
    assert(opportunity.stage === "Prospecting" && opportunity.amount === 1200 && opportunity.accountName === "Acme Inc", "typed fields");
    assert(opportunity.ownerId === null && opportunity.isWon === false, "absent fields");
    assert(opportunity.custom["Region__c"] === "EMEA", "custom field in any case");
    assert(opportunity.custom["Account.Industry"] === "Tech", "custom path");
    assert(opportunity.custom["Description"] === null, "custom null");

    label("responses");
    const record = recordFrom({
        attributes: { type: "Account", url: "/services/data/v68.0/sobjects/Account/001000000000001AAA" },
        Name: "Acme", Owner: { attributes: { type: "User" }, Name: "Dana" }, Parent: null,
        BillingAddress: { city: "Paris", country: "France" },
    });
    assert(record.object === "Account" && record.id === "001000000000001AAA", "type and id from attributes");
    assert(record.fields["Owner.Name"] === "Dana" && record.fields["Parent"] === null, "flattened");
    assert(record.fields["BillingAddress.city"] === "Paris", "compound field");
    assert(cursorFrom({ totalSize: 3000, done: false, nextRecordsUrl: "/services/data/v68.0/query/01gD0000002HU6KIAW-2000", records: [] }) === "01gD0000002HU6KIAW-2000", "cursor");
    assert(cursorFrom({ totalSize: 3, done: true, records: [] }) === null, "last page");
    assert(isCursor("01gD0000002HU6KIAW-2000") && !isCursor("../sobjects/Account") && !isCursor("01gD0000002HU6KIAW-"), "cursor shape");
    assert(pageWindow({ cursor: "01gD0000002HU6KIAW-2000" }).pageSize === 200, "default page size");
    assert(codeOf(() => { pageWindow({ cursor: "/services/data/v68.0/sobjects/User" }); }) === "invalid_cursor", "a path is not a cursor");
    assert(codeOf(() => { pageWindow({ pageSize: 100 }); }) === "invalid_input", "page size below 200");
    const failure = failureFrom(400, "Bad Request", "[{\"message\":\"No such column 'Foo'\",\"errorCode\":\"INVALID_FIELD\",\"fields\":[\"Foo\"]}]");
    assert(failure.errorCode === "INVALID_FIELD" && failure.fields[0] === "Foo" && !failure.retryable, "array error");
    assert(failureFrom(401, "Unauthorized", "{\"error\":\"invalid_grant\",\"error_description\":\"expired\"}").errorCode === "invalid_grant", "OAuth error");
    assert(failureFrom(503, "Unavailable", "<html>").errorCode === "SERVER_ERROR", "HTML");
    assert(failureFrom(403, "Forbidden", "[{\"errorCode\":\"REQUEST_LIMIT_EXCEEDED\",\"message\":\"limit\"}]").retryable, "limits are retryable");
    const outcomes = batchFrom([
        { id: "001000000000001AAA", success: true, errors: [] },
        { success: false, errors: [{ statusCode: "REQUIRED_FIELD_MISSING", message: "Name", fields: ["Name"] }] },
    ]);
    assert(outcomes[0].success && outcomes[1].id === null && outcomes[1].errors[0].errorCode === "REQUIRED_FIELD_MISSING", "batch");

    label("collaboration requests and responses");
    assert(taskSubtype("call") === "Call" && taskSubtype("task") === "Task" && taskSubtype("email") === "Email", "activity types set only the subtype");
    assert(codeOf(() => { taskSubtype("meeting"); }) === "invalid_input", "a meeting is an event, not an activity type");
    const history = activitySoql("Account", "001000000000001AAA", false, 50);
    assert(history.indexOf("FROM ActivityHistories ORDER BY ActivityDate DESC, LastModifiedDate DESC LIMIT 50) FROM Account WHERE Id = '001000000000001AAA'") > 0, history);
    assert(activitySoql("Contact", "003000000000001AAA", true, 5).indexOf("FROM OpenActivities ORDER BY ActivityDate ASC") > 0, "open activities");
    const activities = activitiesFrom({ Id: "001000000000001AAA", ActivityHistories: { totalSize: 1, done: true, records: [
        { attributes: { type: "ActivityHistory" }, Id: "00T000000000001AAA", IsTask: true, Subject: "Call", ActivityDate: "2026-11-02", ActivitySubtype: "Call", IsClosed: true },
    ] } }, false);
    assert(activities.length === 1 && activities[0].isTask && activities[0].subject === "Call" && activities[0].location === null, "activities");
    assert(activitiesFrom({ Id: "001000000000001AAA", ActivityHistories: null }, false).length === 0, "no activities");
    assert(escapeHtml("<b>Q&A</b>") === "&lt;b&gt;Q&amp;A&lt;/b&gt;", "note text is escaped");
    const segments = feedBody("Please review", ["005000000000001AAA"])["messageSegments"] as Record<string, unknown>[];
    assert(segments.length === 3 && segments[2]["type"] === "Mention" && segments[2]["id"] === "005000000000001AAA", "mentions");
    assert(feedCursor("2026-11-18T10:00:00Z,0D5000000000001AAA") === "2026-11-18T10:00:00Z,0D5000000000001AAA", "a feed cursor");
    assert(codeOf(() => { feedCursor("x&pageSize=1000"); }) === "invalid_cursor" && codeOf(() => { feedCursor("../../sobjects"); }) === "invalid_cursor", "a feed cursor cannot change the request");
    const feed = feedPageFrom({ elements: [{
        id: "0D5000000000001AAA", type: "TextPost", createdDate: "2026-11-18T10:00:00.000Z",
        actor: { id: "005000000000001AAA", displayName: "Dana" }, body: { text: "Shipped" },
        capabilities: { comments: { page: { total: 2 } }, chatterLikes: { page: { total: 1 } } },
    }], nextPageToken: "2026-11-18T09:00:00Z,0D5000000000002AAA" });
    assert(feed.posts[0].authorName === "Dana" && feed.posts[0].text === "Shipped" && feed.posts[0].commentCount === 2 && feed.posts[0].likeCount === 1, "feed posts");
    assert(feed.nextCursor === "2026-11-18T09:00:00Z,0D5000000000002AAA" && feedPageFrom({ elements: [], nextPageToken: null }).nextCursor === null, "feed cursor");
    assert(emailAddress(" Dana@Example.COM ") === "dana@example.com", "addresses are lowercased");
    for (const bad of ["dana", "dana@example", "Dana <dana@example.com>", "a@b.com,c@d.com", "a@@b.com", "a@b..com"]) {
        assert(codeOf(() => { emailAddress(bad); }) === "invalid_recipient", "refused " + bad);
    }
    assert(recipients(["a@x.com", "A@x.com", "b@y.org"]).length === 2, "distinct recipients");
    assert(codeOf(() => { recipients(["a@x.com", "b@x.com", "c@x.com", "d@x.com", "e@x.com", "f@x.com"]); }) === "invalid_recipient", "at most five recipients");

    label("process requests and responses");
    const envelope = convertEnvelope("00Dx!token<&>", {
        leadId: "00Q000000000001AAA", accountId: "", contactId: "", convertedStatus: "Closed - Converted",
        createOpportunity: false, opportunityName: "", ownerId: "", sendNotificationEmail: false,
    });
    assert(envelope.indexOf("<urn:sessionId>00Dx!token&lt;&amp;&gt;</urn:sessionId>") > 0, "the token is escaped in the session header");
    assert(envelope.indexOf("<urn:doNotCreateOpportunity>true</urn:doNotCreateOpportunity><urn:leadId>00Q000000000001AAA</urn:leadId>") > 0, "fields in order");
    assert(envelope.indexOf("accountId") < 0 && envelope.indexOf("opportunityName") < 0, "unset fields are left out");
    const converted = convertedFrom(200, "<soapenv:Envelope><soapenv:Body><convertLeadResponse><result><accountId>001000000000001AAA</accountId>" +
        "<contactId>003000000000001AAA</contactId><leadId>00Q000000000001AAA</leadId><opportunityId xsi:nil=\"true\"/><success>true</success>" +
        "</result></convertLeadResponse></soapenv:Body></soapenv:Envelope>");
    assert(converted.accountId === "001000000000001AAA" && converted.contactId === "003000000000001AAA" && converted.opportunityId === null, "converted");
    assert(codeOf(() => { convertedFrom(500, "<soapenv:Fault><faultcode>sf:INVALID_SESSION_ID</faultcode><faultstring>Session expired</faultstring></soapenv:Fault>"); }) === "HTTP 500", "a fault");
    let faultCode = "";
    try {
        convertedFrom(500, "<soapenv:Fault><faultcode>sf:INVALID_SESSION_ID</faultcode><faultstring>Session expired</faultstring></soapenv:Fault>");
    } catch (error: SalesforceError) {
        faultCode = error.errorCode;
    }
    assert(faultCode === "INVALID_SESSION_ID", "a fault's code");
    let convertCode = "";
    try {
        convertedFrom(200, "<result><errors><message>Lead is converted</message><statusCode>CANNOT_UPDATE_CONVERTED_LEAD</statusCode></errors><success>false</success></result>");
    } catch (error: SalesforceError) {
        convertCode = error.errorCode;
    }
    assert(convertCode === "CANNOT_UPDATE_CONVERTED_LEAD", "a conversion error");
    assert(xmlValue("<a:leadIdX>1</a:leadIdX><a:leadId>2</a:leadId>", "leadId") === "2" && xmlValue("<x>1</x>", "leadId") === null, "xml values");
    const approval = approvalFrom([{ instanceId: "04g000000000001AAA", instanceStatus: "Pending", entityId: "006000000000001AAA",
        newWorkitemIds: ["04i000000000001AAA"], actorIds: ["005000000000002AAA"], success: true, errors: null }]);
    assert(approval.status === "Pending" && approval.workItemIds[0] === "04i000000000001AAA" && approval.actorIds.length === 1, "approval");
    const pending = pendingFrom({ Id: "04i000000000001AAA", ActorId: "005000000000002AAA", Actor: { Name: "Lee" },
        ProcessInstance: { TargetObjectId: "006000000000001AAA", TargetObject: { Type: "Opportunity", Name: "Acme" }, ProcessDefinition: { DeveloperName: "Discount" } } });
    assert(pending.object === "Opportunity" && pending.recordName === "Acme" && pending.processName === "Discount" && pending.actorName === "Lee", "pending approval");
    const flow = flowFrom([{ isSuccess: true, outputValues: { caseId: "500000000000001AAA", count: 2, record: { Id: "1" }, Flow__InterviewStatus: "Finished" } }]);
    assert(flow.outputs["caseId"] === "500000000000001AAA" && flow.outputs["count"] === 2 && flow.outputs["record"] === "{\"Id\":\"1\"}", "flow outputs");

    label("reports flatten into rows and totals");
    const tabular = reportFrom({
        attributes: { reportId: "00O000000000001AAA", reportName: "Pipeline" }, allData: true,
        reportMetadata: { id: "00O000000000001AAA", name: "Pipeline", reportFormat: "TABULAR", detailColumns: ["OPPORTUNITY_NAME", "AMOUNT", "CLOSE_DATE"], aggregates: ["s!AMOUNT", "RowCount"], groupingsDown: [], groupingsAcross: [] },
        reportExtendedMetadata: {
            detailColumnInfo: { OPPORTUNITY_NAME: { label: "Opportunity Name", dataType: "string" }, AMOUNT: { label: "Amount", dataType: "currency" }, CLOSE_DATE: { label: "Close Date", dataType: "date" } },
            aggregateColumnInfo: { "s!AMOUNT": { label: "Sum of Amount", dataType: "currency" }, RowCount: { label: "Record Count", dataType: "int" } },
        },
        groupingsDown: { groupings: [] }, groupingsAcross: { groupings: [] },
        factMap: { "T!T": { aggregates: [{ label: "$1,500", value: 1500 }, { label: "2", value: 2 }], rows: [
            { dataCells: [{ label: "Acme", value: "006000000000001AAA" }, { label: "$1,000", value: { amount: 1000, currency: "USD" } }, { label: "11/1/2026", value: "2026-11-01" }] },
            { dataCells: [{ label: "Globex", value: "006000000000002AAA" }, { label: "$500", value: { amount: 500, currency: "USD" } }, { label: "-", value: null }] },
        ] } },
    }, true);
    assert(tabular.rows.length === 2 && tabular.rows[0]["OPPORTUNITY_NAME"] === "Acme" && tabular.rows[0]["AMOUNT"] === 1000, "tabular rows: names and amounts");
    assert(tabular.rows[0]["CLOSE_DATE"] === "2026-11-01" && tabular.rows[1]["CLOSE_DATE"] === null, "dates are values, empty cells null");
    assert(tabular.totals.length === 1 && tabular.totals[0].values["s!AMOUNT"] === 1500 && tabular.totals[0].values["RowCount"] === 2, "grand total");
    assert(tabular.aggregates[0].label === "Sum of Amount" && !tabular.truncated && tabular.raw === null, "aggregate labels");
    const summary = reportFrom({
        allData: false,
        reportMetadata: { id: "00O000000000002AAA", name: "By stage", reportFormat: "SUMMARY", detailColumns: ["OPPORTUNITY_NAME"], aggregates: ["RowCount"], groupingsDown: [{ name: "STAGE_NAME" }, { name: "OWNER" }], groupingsAcross: [] },
        reportExtendedMetadata: { detailColumnInfo: { OPPORTUNITY_NAME: { label: "Name", dataType: "string" } }, aggregateColumnInfo: { RowCount: { label: "Record Count", dataType: "int" } },
            groupingColumnInfo: { STAGE_NAME: { label: "Stage", dataType: "picklist" }, OWNER: { label: "Owner", dataType: "string" } } },
        groupingsDown: { groupings: [
            { key: "0", label: "Prospecting", value: "Prospecting", groupings: [
                { key: "0_0", label: "Dana", value: "005000000000001AAA", groupings: [] },
                { key: "0_1", label: "Lee", value: "005000000000002AAA", groupings: [] },
            ] },
        ] },
        groupingsAcross: { groupings: [] },
        factMap: {
            "0!T": { aggregates: [{ value: 3 }], rows: [] },
            "0_0!T": { aggregates: [{ value: 2 }], rows: [{ dataCells: [{ label: "Acme", value: "006000000000001AAA" }] }, { dataCells: [{ label: "Initech", value: "006000000000003AAA" }] }] },
            "0_1!T": { aggregates: [{ value: 1 }], rows: [{ dataCells: [{ label: "Globex", value: "006000000000002AAA" }] }] },
            "T!T": { aggregates: [{ value: 3 }], rows: [] },
        },
    }, true);
    assert(summary.rows.length === 3 && summary.rows[0]["STAGE_NAME"] === "Prospecting" && summary.rows[0]["OWNER"] === "Dana", "rows carry their groups");
    assert(summary.rows[2]["OWNER"] === "Lee" && summary.rows[2]["OPPORTUNITY_NAME"] === "Globex", "the second group's rows");
    assert(summary.totals.length === 4 && summary.totals[3].values["RowCount"] === 3 && Object.keys(summary.totals[3].groups).length === 0, "grand total last");
    assert(summary.totals[0].groups["STAGE_NAME"] === "Prospecting" && summary.totals[0].groups["OWNER"] === undefined && summary.totals[1].groups["OWNER"] === "Dana", "subtotals at each level");
    assert(summary.truncated && summary.groupings.length === 2, "truncated");
    assert(reportFrom({ reportMetadata: { detailColumns: [] }, factMap: { "T!T": { aggregates: [], rows: [{ dataCells: [] }] } } }, false).rows.length === 0, "totals only");
    const matrix = reportFrom({
        reportMetadata: { reportFormat: "MATRIX", detailColumns: [], aggregates: ["RowCount"], groupingsDown: [{ name: "STAGE_NAME" }], groupingsAcross: [{ name: "CLOSE_MONTH" }] },
        reportExtendedMetadata: { groupingColumnInfo: { STAGE_NAME: { label: "Stage", dataType: "picklist" }, CLOSE_MONTH: { label: "Close Month", dataType: "date" } } },
        groupingsDown: { groupings: [{ key: "0", label: "Won", value: "Closed Won" }] },
        groupingsAcross: { groupings: [{ key: "0", label: "November 2026", value: "2026-11-01" }] },
        factMap: { "0!0": { aggregates: [{ value: 4 }] }, "0!T": { aggregates: [{ value: 4 }] }, "T!0": { aggregates: [{ value: 4 }] }, "T!T": { aggregates: [{ value: 4 }] } },
    }, true);
    assert(matrix.totals.length === 4 && matrix.totals[0].groups["STAGE_NAME"] === "Won" && matrix.totals[0].groups["CLOSE_MONTH"] === "2026-11-01", "matrix cells");
    const description = reportDescriptionFrom({ reportMetadata: { id: "00O000000000001AAA", name: "Pipeline", reportFormat: "TABULAR", reportType: { type: "Opportunity" },
        detailColumns: ["AMOUNT"], aggregates: [], groupingsDown: [], reportFilters: [{ column: "STAGE_NAME", operator: "notEqual", value: "Closed Lost" }], reportBooleanFilter: null } });
    assert(description.reportType === "Opportunity" && description.filters[0].operator === "notEqual" && description.booleanFilter === null, "describe");
    assert(reportFilter({ column: "Account.Industry", operator: "equals", value: "Tech" })["column"] === "Account.Industry", "a filter");
    assert(codeOf(() => { reportFilter({ column: "AMOUNT;", operator: "equals", value: "1" }); }) === "invalid_input", "a filter's column");
    const filtered = withFilters({ reportFilters: [{ column: "A" }, { column: "B" }], reportBooleanFilter: "1 OR 2", name: "x" }, [{ column: "C" }]);
    assert((filtered["reportFilters"] as unknown[]).length === 3 && filtered["reportBooleanFilter"] === "(1 OR 2) AND 3" && filtered["name"] === "x", "added filters keep the report's logic");
    assert(withFilters({ reportFilters: [] }, [{ column: "C" }])["reportBooleanFilter"] === undefined, "no logic to extend");
    const dashboard = dashboardFrom({
        dashboardMetadata: { id: "01Z000000000001AAA", name: "Sales", runningUser: { id: "005000000000001AAA" }, components: [
            { id: "c1", header: "Pipeline", reportId: "00O000000000002AAA", properties: { visualizationType: "Bar" } },
            { id: "c2", header: "Empty", reportId: "00O000000000003AAA" },
        ] },
        componentData: [{ componentId: "c1", status: { refreshDate: "2026-11-18T08:00:00Z" }, reportResult: {
            reportMetadata: { aggregates: ["RowCount"], groupingsDown: [{ name: "STAGE_NAME" }] },
            groupingsDown: { groupings: [{ key: "0", label: "Prospecting", value: "Prospecting" }] },
            factMap: { "0!T": { aggregates: [{ value: 3 }] }, "T!T": { aggregates: [{ value: 3 }] } },
        } }],
    });
    assert(dashboard.components[0].type === "Bar" && dashboard.components[0].totals.length === 2 && dashboard.components[0].refreshDate !== null, "dashboard component");
    assert(dashboard.components[1].totals.length === 0 && dashboard.runningUserId === "005000000000001AAA", "a component without data");

    label("metadata");
    const described = objectDescriptionFrom({ name: "Opportunity", label: "Opportunity", keyPrefix: "006", fields: [
        { name: "StageName", label: "Stage", type: "picklist", filterable: true, picklistValues: [{ value: "Prospecting", active: true }, { value: "Old", active: false }] },
        { name: "AccountId", type: "reference", referenceTo: ["Account"], relationshipName: "Account" },
    ], childRelationships: [{ relationshipName: "OpportunityLineItems", childSObject: "OpportunityLineItem", field: "OpportunityId" }], recordTypeInfos: [] });
    assert(described.fields[0].picklistValues.length === 1 && described.fields[1].relationshipName === "Account" && described.fields[1].referenceTo[0] === "Account", "fields");
    assert(described.childRelationships[0].childObject === "OpportunityLineItem", "child relationships");
    const limits = limitsFrom({ DailyApiRequests: { Max: 15000, Remaining: 14000 }, DataStorageMB: { Max: 5, Remaining: 5 } });
    assert(limits[0].name === "DailyApiRequests" && limits[0].remaining === 14000, "limits");

    if (secrets.get("SALESFORCE_ACCESS_TOKEN") !== undefined) return;

    label("operations validate their input before any request");
    const id = "006000000000001";
    assert(codeOf(() => { getRecord("Opportunity;", id); }) === "invalid_name", "object");
    assert(codeOf(() => { getRecord("Opportunity", "006"); }) === "invalid_id", "id");
    assert(codeOf(() => { createRecord("Account", []); }) === "invalid_input", "empty create");
    assert(codeOf(() => { updateRecord("Opportunity", id, [{ field: "OwnerId", value: "005000000000001" }]); }) === "owner_field", "owner change");
    assert(codeOf(() => { upsertRecord("Account", "Ext__c", "1", [{ field: "Ext__c", value: "2" }]); }) === "invalid_input", "upsert sets its key");
    assert(codeOf(() => { updateRecord("Contact", "003000000000001", [{ field: "AccountId", value: "001000000000001" }]); }) === "account_field", "an account change");
    assert(codeOf(() => { upsertRecord("Contact", "Ext__c", "1", [{ field: "AccountId", value: "001000000000001" }]); }) === "account_field", "an upsert's account");
    assert(codeOf(() => { createRecord("Task", [{ field: "WhatId", value: "006000000000001" }]); }) === "account_field", "a generic task's record");
    assert(codeOf(() => { updateOpportunity(id, { custom: [{ field: "StageName", value: "Closed Won" }] }); }) === "typed_field", "a stage through custom");
    assert(codeOf(() => { updateOpportunity(id, { custom: [{ field: "AccountId", value: "001000000000001AAA" }] }); }) === "typed_field", "a deal's account through custom");
    assert(codeOf(() => { createContact({ lastName: "Lee", custom: [{ field: "AccountId", value: "001000000000001" }] }); }) === "typed_field", "a contact's account through custom");
    assert(codeOf(() => { logActivity({ type: "call", subject: "Call", whoId: "003000000000001", custom: [{ field: "WhatId", value: "006000000000001" }] }); }) === "typed_field", "an activity's record through custom");
    assert(codeOf(() => { createRecords("Account", []); }) === "invalid_input", "empty batch");
    assert(codeOf(() => { updateRecords("Account", [{ id: id, values: [{ field: "Name", value: "a" }] }, { id: id, values: [{ field: "Name", value: "b" }] }]); }) === "invalid_input", "a repeated id");
    assert(codeOf(() => { findOpportunities({ closeAfter: "next week" }); }) === "invalid_date", "a filter");
    assert(codeOf(() => { findOpportunities({}, { pageSize: 5000 }); }) === "invalid_input", "a page size");
    assert(codeOf(() => { getAccount(id, { fields: ["Region__c)"] }); }) === "invalid_name", "an extra field");
    assert(codeOf(() => { updateOpportunity(id, { stage: " " }); }) === "invalid_input", "a blank stage");
    assert(codeOf(() => { updateOpportunity(id, { closeDate: "11/01/2026" }); }) === "invalid_date", "a close date");
    assert(codeOf(() => { createCase({ subject: "" }); }) === "invalid_input", "a blank subject");
    assert(codeOf(() => { query({ object: "Account", fields: ["Name"], where: [{ field: "Name", op: "in", value: "x" }] }); }) === "invalid_condition", "a query");
    assert(codeOf(() => { aggregate({ object: "Account", measures: [] }); }) === "invalid_input", "an aggregate");
    assert(codeOf(() => { search({ text: "x", objects: [{ object: "Account" }] }); }) === "invalid_input", "a search");
    assert(codeOf(() => { exportQuery({ object: "Account", fields: ["Name"], orderBy: [{ field: "Name" }] }, "/a.csv"); }) === "invalid_input", "an export with orderBy");
    assert(codeOf(() => { logActivity({ type: "call", subject: " " }); }) === "invalid_input", "an activity's subject");
    assert(codeOf(() => { logActivity({ type: "call", subject: "Call", date: "tomorrow" }); }) === "invalid_date", "an activity's date");
    assert(codeOf(() => { createEvent({ subject: "Demo", start: "2026-11-20 10:00", end: "2026-11-20T11:00:00Z" }); }) === "invalid_date", "an event's start");
    assert(codeOf(() => { listActivities(id, { limit: 1000 }); }) === "invalid_input", "an activity limit");
    assert(codeOf(() => { createNote(id, "", "text"); }) === "invalid_input", "a note's title");
    assert(codeOf(() => { uploadFile(id, "/dir/"); }) === "invalid_input", "an upload path");
    assert(codeOf(() => { downloadFile("069", "/a.pdf"); }) === "invalid_id", "a file id");
    assert(codeOf(() => { postToFeed(id, "hi", { mentions: ["Dana"] }); }) === "invalid_id", "a mention");
    assert(codeOf(() => { readFeed(id, { pageSize: 500 }); }) === "invalid_input", "a feed page size");
    assert(codeOf(() => { sendEmail({ to: ["Dana <dana@example.com>"], subject: "Hi", body: "Hello" }); }) === "invalid_recipient", "an email recipient");
    assert(codeOf(() => { submitForApproval(id, { skipEntryCriteria: true }); }) === "invalid_input", "skipping criteria without a process");
    assert(codeOf(() => { approve("04i"); }) === "invalid_id", "a work item");
    assert(codeOf(() => { listPendingApprovals({ actorId: "me" }); }) === "invalid_id", "an actor");
    assert(codeOf(() => { runFlow("Flow.Name"); }) === "invalid_name", "a flow name");
    assert(codeOf(() => { runFlow("Escalate", [{ name: "caseId", value: "1" }, { name: "CASEID", value: "2" }]); }) === "duplicate_field", "a repeated flow input");
    assert(codeOf(() => { convertLead("00Q000000000001", { contactId: "003000000000001" }); }) === "invalid_input", "a contact without its account");
    assert(codeOf(() => { convertLead("00Q000000000001", { createOpportunity: false, opportunityName: "Deal" }); }) === "invalid_input", "an opportunity name without one");
    assert(codeOf(() => { changeOwner("Opportunity", id, "Dana"); }) === "invalid_id", "a new owner");
    assert(codeOf(() => { runReport("00O000000000001", { filters: [{ column: "AMOUNT) OR (1", operator: "equals", value: "1" }] }); }) === "invalid_input", "a report filter");
    assert(codeOf(() => { describeReport("report"); }) === "invalid_id", "a report id");
    assert(codeOf(() => { runListView("Account", "00B000000000001", { cursor: "next" }); }) === "invalid_cursor", "a list view cursor");

    label("valid input stops at the missing credential");
    for (const action of [
        () => { getRecord("Opportunity", id); },
        () => { findOpportunities({ stage: "Prospecting" }); },
        () => { query({ object: "Account", fields: ["Name"] }); },
        () => { updateOpportunity(id, { stage: "Closed Won" }); },
        () => { logActivity({ type: "call", subject: "Call", whatId: id }); },
        () => { sendEmail({ to: ["dana@example.com"], subject: "Hi", body: "Hello" }); },
        () => { runFlow("Escalate", [{ name: "caseId", value: "500000000000001" }]); },
        () => { runReport("00O000000000001", { filters: [{ column: "STAGE_NAME", operator: "equals", value: "Won" }] }); },
    ]) {
        const code = codeOf(action);
        assert(code === "missing_token", "stopped at the credential, got " + code);
    }
}
