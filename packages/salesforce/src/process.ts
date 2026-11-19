// Process and automation: approvals, flows and lead conversion. The input and
// result types, the SOAP request for `convertLead` (Salesforce has no REST
// equivalent) and its parser. Internal: `src/lib.ts` re-exports the types.

import { SalesforceError } from "./errors";
import { FieldValue } from "./request";
import { arrayAt, objectAt, scalarOrJson, textAt } from "./response";

/** Options of `submitForApproval`. */
export interface SubmitOptions {
    /** A comment for the approver. */
    comment?: string;
    /** The approval process's API name; default the first process whose entry criteria the record meets. */
    processName?: string;
    /** The first approver, when the process lets the submitter choose. */
    nextApproverIds?: string[];
    /** Skip the process's entry criteria; needs `processName`. */
    skipEntryCriteria?: boolean;
}

/** Options of `approve` and `reject`. */
export interface DecisionOptions {
    /** A comment recorded with the decision. */
    comment?: string;
    /** The next approver, when the process lets the approver choose. */
    nextApproverIds?: string[];
}

/** The outcome of an approval request. */
export interface ApprovalResult {
    /** The approval process instance. */
    instanceId: string;
    /** `Pending`, `Approved`, `Rejected` or `Removed`. */
    status: string;
    /** The record the approval is about. */
    recordId: string;
    /** Work items now waiting for an approver; pass one to `approve` or `reject`. */
    workItemIds: string[];
    /** The users or queues asked to approve next. */
    actorIds: string[];
}

/** Options of `listPendingApprovals`. */
export interface PendingOptions {
    /** Only work items assigned to this user or queue. */
    actorId?: string;
    /** Records per page, 200 to 2000; default 200. */
    pageSize?: number;
    /** A page's `nextCursor`, for the next page. */
    cursor?: string;
}

/** An approval waiting for a decision. */
export interface PendingApproval {
    /** The work item; pass it to `approve` or `reject`. */
    workItemId: string;
    /** The approval process instance. */
    instanceId: string | null;
    /** The record to approve. */
    recordId: string | null;
    /** Its name. */
    recordName: string | null;
    /** Its object API name. */
    object: string | null;
    /** The approval process's API name. */
    processName: string | null;
    /** The user or queue asked to approve. */
    actorId: string | null;
    /** Their name. */
    actorName: string | null;
    /** Who submitted the record. */
    submittedById: string | null;
    /** When the work item was created. */
    createdDate: string | null;
}

/** One input variable of a flow. */
export interface FlowInput {
    /** The variable's API name, as the flow defines it (available for input). */
    name: string;
    /** Its value. */
    value: FieldValue;
}

/** The outcome of a flow run. */
export interface FlowResult {
    /** The flow's output variables by name; a record or collection is its JSON text. */
    outputs: Record<string, FieldValue>;
}

/** Options of `convertLead`. */
export interface ConvertOptions {
    /** Merge into this existing account; default a new account from the lead's company. */
    accountId?: string;
    /** Merge into this existing contact (of `accountId`); default a new contact. */
    contactId?: string;
    /** Create an opportunity; default true. */
    createOpportunity?: boolean;
    /** The new opportunity's name; default the lead's company. */
    opportunityName?: string;
    /** The converted lead's status; default the org's first converted status. */
    convertedStatus?: string;
    /** The owner of the new records; default the lead's owner. */
    ownerId?: string;
    /** Email the new owner; default false. */
    sendNotificationEmail?: boolean;
}

/** What `convertLead` created or used. */
export interface ConvertedLead {
    /** The lead. */
    leadId: string;
    /** The account. */
    accountId: string;
    /** The contact. */
    contactId: string;
    /** The opportunity, or null when none was created. */
    opportunityId: string | null;
}

/** A conversion, validated, in the SOAP API's terms. */
export interface LeadConvert {
    leadId: string;
    accountId: string;
    contactId: string;
    convertedStatus: string;
    createOpportunity: boolean;
    opportunityName: string;
    ownerId: string;
    sendNotificationEmail: boolean;
}

/**
 * The body of `POST process/approvals`.
 * @param request One request: `actionType` and its fields.
 * @returns The body.
 */
export function approvalBody(request: Record<string, unknown>): Record<string, unknown> {
    return { requests: [request] };
}

/**
 * The outcome of one approval request.
 * @param results Salesforce's answer, one result per request.
 * @returns The outcome.
 */
export function approvalFrom(results: unknown[]): ApprovalResult {
    const first = results.length > 0 ? results[0] : null;
    const ids = (key: string): string[] => {
        const out: string[] = [];
        for (const item of arrayAt(first, key)) {
            if (typeof item === "string") out.push(item);
        }
        return out;
    };
    return {
        instanceId: textAt(first, "instanceId") ?? "",
        status: textAt(first, "instanceStatus") ?? "",
        recordId: textAt(first, "entityId") ?? "",
        workItemIds: ids("newWorkitemIds"),
        actorIds: ids("actorIds"),
    };
}

/**
 * A pending work item, from `ProcessInstanceWorkitem`.
 * @param raw The row as Salesforce returned it.
 * @returns The pending approval.
 */
export function pendingFrom(raw: Record<string, unknown>): PendingApproval {
    const instance = objectAt(raw, "ProcessInstance");
    const target = objectAt(instance, "TargetObject");
    return {
        workItemId: textAt(raw, "Id") ?? "",
        instanceId: textAt(raw, "ProcessInstanceId"),
        recordId: textAt(instance, "TargetObjectId"),
        recordName: textAt(target, "Name"),
        object: textAt(target, "Type"),
        processName: textAt(objectAt(instance, "ProcessDefinition"), "DeveloperName"),
        actorId: textAt(raw, "ActorId"),
        actorName: textAt(objectAt(raw, "Actor"), "Name"),
        submittedById: textAt(instance, "SubmittedById"),
        createdDate: textAt(raw, "CreatedDate"),
    };
}

export const PENDING_FIELDS =
    "Id, ActorId, Actor.Name, CreatedDate, ProcessInstanceId, ProcessInstance.TargetObjectId, ProcessInstance.TargetObject.Name, " +
    "ProcessInstance.TargetObject.Type, ProcessInstance.ProcessDefinition.DeveloperName, ProcessInstance.SubmittedById";

/**
 * A flow run's output variables.
 * @param results Salesforce's answer, one result per input set.
 * @returns The outputs.
 */
export function flowFrom(results: unknown[]): FlowResult {
    const first = results.length > 0 ? results[0] : null;
    const values = objectAt(first, "outputValues");
    const outputs: Record<string, FieldValue> = {};
    for (const key of Object.keys(values)) outputs[key] = scalarOrJson(values[key]);
    return { outputs: outputs };
}

/**
 * The SOAP envelope of a `convertLead` call. The token travels in the session
 * header, which is how the SOAP API takes an OAuth access token.
 * @param token The access token.
 * @param convert The conversion.
 * @returns The XML request.
 */
export function convertEnvelope(token: string, convert: LeadConvert): string {
    // Elements in the order the partner WSDL's LeadConvert type declares them.
    let fields = "";
    if (convert.accountId.length > 0) fields += element("accountId", convert.accountId);
    if (convert.contactId.length > 0) fields += element("contactId", convert.contactId);
    fields += element("convertedStatus", convert.convertedStatus);
    fields += element("doNotCreateOpportunity", convert.createOpportunity ? "false" : "true");
    fields += element("leadId", convert.leadId);
    if (convert.createOpportunity && convert.opportunityName.length > 0) fields += element("opportunityName", convert.opportunityName);
    fields += element("overwriteLeadSource", "false");
    if (convert.ownerId.length > 0) fields += element("ownerId", convert.ownerId);
    fields += element("sendNotificationEmail", convert.sendNotificationEmail ? "true" : "false");
    return "<?xml version=\"1.0\" encoding=\"UTF-8\"?>" +
        "<soapenv:Envelope xmlns:soapenv=\"http://schemas.xmlsoap.org/soap/envelope/\" xmlns:urn=\"urn:partner.soap.sforce.com\">" +
        "<soapenv:Header><urn:SessionHeader><urn:sessionId>" + escapeXml(token) + "</urn:sessionId></urn:SessionHeader></soapenv:Header>" +
        "<soapenv:Body><urn:convertLead><urn:leadConverts>" + fields + "</urn:leadConverts></urn:convertLead></soapenv:Body>" +
        "</soapenv:Envelope>";
}

function element(name: string, value: string): string {
    return "<urn:" + name + ">" + escapeXml(value) + "</urn:" + name + ">";
}

export function escapeXml(text: string): string {
    let out = "";
    for (let i = 0; i < text.length; i += 1) {
        const char = text.charAt(i);
        if (char === "&") out += "&amp;";
        else if (char === "<") out += "&lt;";
        else if (char === ">") out += "&gt;";
        else if (char === "\"") out += "&quot;";
        else if (char === "'") out += "&apos;";
        else out += char;
    }
    return out;
}

/**
 * Decode a `convertLead` answer: the result, a conversion error, or a SOAP fault.
 * @param status HTTP status.
 * @param xml The response body.
 * @returns What the conversion created or used.
 */
export function convertedFrom(status: number, xml: string): ConvertedLead {
    const fault = xmlValue(xml, "faultcode");
    if (fault !== null) {
        const colon = fault.indexOf(":");
        const code = colon >= 0 ? fault.slice(colon + 1) : fault;
        const message = xmlValue(xml, "faultstring") ?? "";
        throw new SalesforceError(code, "Salesforce " + status.toString() + " " + code + ": " + message, [], status, code === "REQUEST_LIMIT_EXCEEDED");
    }
    if (xmlValue(xml, "success") !== "true") {
        const code = xmlValue(xml, "statusCode") ?? (status >= 200 && status < 300 ? "CONVERT_FAILED" : "HTTP_" + status.toString());
        const message = xmlValue(xml, "message") ?? "the lead was not converted";
        const field = xmlValue(xml, "fields");
        throw new SalesforceError(code, "Salesforce " + status.toString() + " " + code + ": " + message, field === null || field.length === 0 ? [] : [field], status, code === "UNABLE_TO_LOCK_ROW");
    }
    const opportunity = xmlValue(xml, "opportunityId");
    return {
        leadId: xmlValue(xml, "leadId") ?? "",
        accountId: xmlValue(xml, "accountId") ?? "",
        contactId: xmlValue(xml, "contactId") ?? "",
        opportunityId: opportunity === null || opportunity.length === 0 ? null : opportunity,
    };
}

/**
 * The text of the first element named `tag`, with or without a namespace
 * prefix; "" for an empty or nil element; null when there is none.
 * @param xml The document.
 * @param tag The local name.
 * @returns The element's text, unescaped.
 */
export function xmlValue(xml: string, tag: string): string | null {
    let from = 0;
    while (true) {
        const at = xml.indexOf(tag, from);
        if (at < 0) return null;
        from = at + tag.length;
        const after = xml.charAt(at + tag.length);
        if (after !== ">" && after !== " " && after !== "/") continue;
        let start = at - 1;
        if (xml.charAt(start) === ":") {
            start -= 1;
            while (start >= 0 && isNameChar(xml.charCodeAt(start))) start -= 1;
        }
        if (start < 0 || xml.charAt(start) !== "<") continue;
        const close = xml.indexOf(">", at);
        if (close < 0) return null;
        if (xml.charAt(close - 1) === "/") return "";
        const end = xml.indexOf("<", close + 1);
        if (end < 0) return null;
        return unescapeXml(xml.slice(close + 1, end));
    }
}

function isNameChar(code: number): boolean {
    return (code >= 65 && code <= 90) || (code >= 97 && code <= 122) || (code >= 48 && code <= 57) || code === 95 || code === 45;
}

function unescapeXml(text: string): string {
    return text.split("&lt;").join("<").split("&gt;").join(">").split("&quot;").join("\"").split("&apos;").join("'").split("&amp;").join("&");
}
