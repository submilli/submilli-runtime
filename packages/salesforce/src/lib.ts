// A synchronous, typed client for the Salesforce REST API: records of any
// object, typed standard objects, queries, aggregates and search written from
// typed descriptions, collaboration (activities, notes, files, Chatter, email),
// process (approvals, flows, lead conversion, ownership), and reports,
// dashboards and metadata. Requests go to the org chosen when the package is
// installed (the `orgUrl` build setting); the access token is read from the
// SALESFORCE_ACCESS_TOKEN secret inside the package, is never an argument and
// is never returned.
//
// Every operation normalizes its input, checks the normalized values, and sends
// exactly those values. Facts a check needs that the caller does not supply (a
// record's owner and account) are read from Salesforce first, never taken from
// the caller; a record that does not exist is checked with "" for them, so a
// denial does not reveal whether it exists. `OwnerId` is refused in updates: an
// owner-scoped rule cannot be bypassed by reassigning a record, and
// `changeOwner` is the one checked way to reassign.

import { delete as httpDelete, download, get, patch, post, Response } from "submilli:http";
import { appendText, exists, read, remove, stat, writeText } from "submilli:fs";
import { check } from "submilli:security";
import { encodeComponent } from "submilli:url";
import { apiVersion, orgUrl } from "./config";
import { SalesforceError, validationError } from "./errors";
import { accessToken } from "./org";
import {
    ApiName,
    FieldValue,
    idAmong,
    normalizeId,
    optionalId,
    normalizeName,
    PageOptions,
    PageWindow,
    pageWindow,
    FieldWrite,
    prepareValues,
    requireDateTime,
    requireText,
    sortedDistinct,
} from "./request";
import {
    AggregateOrder,
    AggregateSpec,
    buildAggregate,
    buildQuery,
    buildSearch,
    Condition,
    containsLiteral,
    GroupKey,
    Measure,
    OrderBy,
    PreparedCondition,
    QuerySpec,
    quote,
    SearchObject,
    SearchSpec,
} from "./soql";
import {
    Account,
    AccountFilters,
    accountFrom,
    AccountRecordFilters,
    Campaign,
    CampaignFilters,
    campaignFrom,
    CampaignMember,
    CampaignMemberFilters,
    campaignMemberFrom,
    Case,
    CaseFilters,
    caseFrom,
    CaseInput,
    CaseUpdate,
    Contact,
    ContactFilters,
    contactFrom,
    ContactInput,
    Contract,
    contractFrom,
    Event,
    EventFilters,
    eventFrom,
    ExtraFields,
    extraFields,
    Filters,
    Lead,
    LeadFilters,
    leadFrom,
    LeadInput,
    LeadUpdate,
    lineItemFrom,
    Opportunity,
    OpportunityFilters,
    opportunityFrom,
    OpportunityLineItem,
    OpportunityLineItemFilters,
    OpportunityUpdate,
    Order,
    orderFrom,
    Pricebook,
    PricebookFilters,
    pricebookFrom,
    Product,
    ProductFilters,
    productFrom,
    Task,
    TaskFilters,
    taskFrom,
    typedSoql,
    User,
    UserFilters,
    userFrom,
    Writes,
} from "./objects";
import {
    aggregateFrom,
    AggregateResult,
    ApiDeleted,
    ApiQueryResult,
    ApiSaveResult,
    ApiSearchResult,
    ApiUpdated,
    BatchOutcome,
    batchFrom,
    DeletedRecord,
    DeletedRecords,
    actionFailure,
    failureFrom,
    FailureDetails,
    fieldsOf,
    flagAt,
    objectAt,
    Page,
    pageFrom,
    recordFrom,
    SalesforceRecord,
    statusCode,
    textAt,
    textField,
    UpdatedRecords,
    UpsertResult,
} from "./response";

import {
    Activity,
    activitiesFrom,
    activityFields,
    activityLimit,
    ActivityInput,
    ActivityOptions,
    activitySoql,
    domainOf,
    DownloadedFile,
    EmailInput,
    EventInput,
    feedBody,
    feedCursor,
    FeedOptions,
    FeedPage,
    feedPageFrom,
    feedPageSize,
    FileDownloadOptions,
    fileName,
    noteContent,
    PostOptions,
    recipients,
    UploadedFile,
    UploadOptions,
} from "./activity";
import {
    approvalBody,
    approvalFrom,
    ApprovalResult,
    ConvertedLead,
    convertedFrom,
    convertEnvelope,
    ConvertOptions,
    DecisionOptions,
    FlowInput,
    flowFrom,
    FlowResult,
    PENDING_FIELDS,
    PendingApproval,
    pendingFrom,
    PendingOptions,
    SubmitOptions,
} from "./process";
import {
    Dashboard,
    dashboardFrom,
    DashboardSummary,
    ListOptions,
    reportDescriptionFrom,
    ReportDescription,
    reportFilter,
    reportFrom,
    ReportResult,
    ReportSummary,
    RunReportOptions,
    withFilters,
} from "./reports";
import {
    CurrentUser,
    currentUserFrom,
    ListView,
    listViewsFrom,
    limitsFrom,
    ObjectDescription,
    objectDescriptionFrom,
    objectsFrom,
    ObjectSummary,
    OrgLimit,
} from "./metadata";

export { SalesforceError } from "./errors";
export { FieldValue, FieldWrite, PageOptions } from "./request";
export {
    ConditionOperator,
    Condition,
    OrderBy,
    QuerySpec,
    MeasureFunction,
    Measure,
    DateGroup,
    GroupKey,
    AggregateOrder,
    AggregateSpec,
    SearchObject,
    SearchSpec,
} from "./soql";
export {
    StandardRecord,
    Account,
    Contact,
    Lead,
    Opportunity,
    OpportunityLineItem,
    Product,
    Pricebook,
    Case,
    Task,
    Event,
    User,
    Campaign,
    CampaignMember,
    Contract,
    Order,
    AccountFilters,
    ContactFilters,
    LeadFilters,
    OpportunityFilters,
    CaseFilters,
    OpportunityLineItemFilters,
    ProductFilters,
    PricebookFilters,
    TaskFilters,
    EventFilters,
    UserFilters,
    CampaignFilters,
    CampaignMemberFilters,
    AccountRecordFilters,
    ContactInput,
    LeadInput,
    LeadUpdate,
    OpportunityUpdate,
    CaseInput,
    CaseUpdate,
} from "./objects";
export {
    SalesforceRecord,
    Page,
    AggregateRow,
    AggregateResult,
    BatchOutcome,
    RecordError,
    UpsertResult,
    UpdatedRecords,
    DeletedRecords,
    DeletedRecord,
} from "./response";

export {
    ActivityType,
    ActivityInput,
    EventInput,
    ActivityOptions,
    Activity,
    UploadOptions,
    UploadedFile,
    FileDownloadOptions,
    DownloadedFile,
    PostOptions,
    FeedOptions,
    FeedPost,
    FeedPage,
    EmailInput,
} from "./activity";
export {
    SubmitOptions,
    DecisionOptions,
    ApprovalResult,
    PendingOptions,
    PendingApproval,
    FlowInput,
    FlowResult,
    ConvertOptions,
    ConvertedLead,
} from "./process";
export {
    ReportOperator,
    ReportFilter,
    SavedFilter,
    RunReportOptions,
    ReportColumn,
    ReportTotal,
    ReportResult,
    ReportSummary,
    ReportDescription,
    ListOptions,
    DashboardSummary,
    Dashboard,
    DashboardComponent,
} from "./reports";
export {
    ObjectSummary,
    ObjectDescription,
    FieldDescription,
    ChildRelationship,
    RecordType,
    ListView,
    OrgLimit,
    CurrentUser,
} from "./metadata";

// The org's URL, set when the package is installed, and the slash that ends its host.
const ORG = orgUrl + "/";
const USER_AGENT = "submilli-salesforce/0.1.0";
const MAX_BATCH = 200;
const EXPORT_POLLS = 30;
const EXPORT_CHUNK_RECORDS = 10000;
const EXPORT_MAX_CHUNKS = 500;
// Salesforce takes at most 37.5 MB of file data in a JSON body (50 MB once base64-encoded).
const MAX_UPLOAD_BYTES = 37 * 1000 * 1000;

/** Options of `getRecord`. */
export interface GetRecordOptions {
    /** Fields to return; default every field the user may read. */
    fields?: string[];
}

/** Options of the typed gets: extra fields, such as custom fields, returned in `custom`. */
export interface ReadOptions {
    /** Field API names or relationship paths, such as `Region__c` or `Account.Industry`. */
    fields?: string[];
}

/** Options of the typed finds: extra fields and the page. */
export interface FindOptions extends PageOptions {
    /** Field API names or relationship paths, returned in each record's `custom`. */
    fields?: string[];
}

/** Options of `getRelated`. */
export interface RelatedOptions extends PageOptions {
    /** Fields of the related records; default every field the user may read. */
    fields?: string[];
}

/** One record of `updateRecords`. */
export interface RecordUpdate {
    /** Record id. */
    id: string;
    /** The fields to change. */
    values: FieldWrite[];
}

/** Options of `exportQuery`. */
export interface ExportOptions {
    /** The `jobId` of an export that was not done, to continue it. Pass the same query and path with it. */
    jobId?: string;
}

/** The outcome of `exportQuery`. */
export interface ExportResult {
    /** The Bulk API job's id. */
    jobId: string;
    /** `UploadComplete`, `InProgress` or `JobComplete`. */
    state: string;
    /** True when the CSV file is written. While false, call `exportQuery` again with `{ jobId }`. */
    done: boolean;
    /** The CSV file, once written. */
    path: string;
    /** Rows written, without the header. */
    records: number;
}

// ---------------------------------------------------------------------------
// Records of any object
// ---------------------------------------------------------------------------

/**
 * Get one record of any object; null when it does not exist.
 * @param object Object API name, such as `Opportunity` or `Invoice__c`.
 * @param id Record id (15 or 18 characters).
 * @param options `fields` to return; default every field the user may read.
 * @returns The record, or null.
 * @capability salesforce.com/records.get { object: string, ownerId: string, accountId: string }
 */
export function getRecord(object: string, id: string, options: GetRecordOptions = {}): SalesforceRecord | null {
    const name = normalizeName(object, "object");
    const recordId = normalizeId(id, "id");
    const fields: string[] = [];
    for (const field of options.fields ?? []) fields.push(normalizeName(field, "fields").name);
    const raw = fetchRecord(name.name, recordId);
    check("salesforce.com/records.get", { object: name.key, ownerId: ownerOf(raw), accountId: accountOf(name.key, recordId, raw) });
    if (raw === null) return null;
    const record = recordFrom(raw);
    if (fields.length === 0) return record;
    const projected: Record<string, FieldValue> = {};
    const keys = Object.keys(record.fields);
    for (const field of fields) {
        let value: FieldValue = null;
        for (const key of keys) {
            if (key.toLowerCase() === field.toLowerCase()) value = record.fields[key];
        }
        projected[field] = value;
    }
    return { object: record.object, id: record.id, fields: projected };
}

/**
 * Create a record of any object.
 * @param object Object API name.
 * @param values The fields to write, by API name. `OwnerId` sets the owner (default the token's user).
 * @returns The new record's id.
 * @capability salesforce.com/records.create { object: string, fields: string[], ownerId: string, accountId: string }
 */
export function createRecord(object: string, values: FieldWrite[]): string {
    const name = normalizeName(object, "object");
    const given: FieldWrite[] = [];
    for (const write of values) {
        const { field, value } = write;
        given.push({ field: field, value: value });
    }
    const prepared = prepareValues(given, "values", false);
    if (prepared.keys.length === 0) throw validationError("invalid_input", "values cannot be empty");
    const ownerId = idAmong(given, "ownerid", "values");
    const accountId = name.key === "account" ? "" : idAmong(given, "accountid", "values");
    check("salesforce.com/records.create", { object: name.key, fields: prepared.keys, ownerId: ownerId, accountId: accountId });
    return createdId(sfPost("/sobjects/" + name.name, prepared.body));
}

/**
 * Update a record of any object. Only the fields given change; `null` clears a
 * field. `OwnerId` is refused.
 * @param object Object API name.
 * @param id Record id.
 * @param values The fields to write, by API name.
 * @capability salesforce.com/records.update { object: string, fields: string[], ownerId: string, accountId: string }
 */
export function updateRecord(object: string, id: string, values: FieldWrite[]): void {
    const name = normalizeName(object, "object");
    const recordId = normalizeId(id, "id");
    const given: FieldWrite[] = [];
    for (const write of values) {
        const { field, value } = write;
        given.push({ field: field, value: value });
    }
    const prepared = prepareValues(given, "values", true);
    if (prepared.keys.length === 0) throw validationError("invalid_input", "values cannot be empty");
    const raw = fetchRecord(name.name, recordId);
    check("salesforce.com/records.update", {
        object: name.key, fields: prepared.keys, ownerId: ownerOf(raw), accountId: accountOf(name.key, recordId, raw),
    });
    sfPatch("/sobjects/" + name.name + "/" + recordId, prepared.body);
}

/**
 * Delete a record of any object (Salesforce moves it to the recycle bin).
 * @param object Object API name.
 * @param id Record id.
 * @returns True when it was deleted, false when it did not exist.
 * @capability salesforce.com/records.delete { object: string, ownerId: string, accountId: string }
 */
export function deleteRecord(object: string, id: string): boolean {
    const name = normalizeName(object, "object");
    const recordId = normalizeId(id, "id");
    const raw = fetchRecord(name.name, recordId);
    check("salesforce.com/records.delete", { object: name.key, ownerId: ownerOf(raw), accountId: accountOf(name.key, recordId, raw) });
    if (raw === null) return false;
    const response = orgDelete("/sobjects/" + name.name + "/" + recordId);
    if (response.ok) return true;
    const error = errorOf(response);
    if (error.status === 404) return false;
    throw error;
}

/**
 * Create a record, or update the one whose external id field has this value.
 * The fields given replace an existing record's values; `OwnerId` is refused.
 * @param object Object API name.
 * @param externalIdField An external id field, such as `Customer_Number__c`.
 * @param externalId Its value.
 * @param values The fields to write, by API name.
 * @returns The record's id and whether it was created.
 * @capability salesforce.com/records.upsert { object: string, externalIdField: string, externalId: string, fields: string[] }
 */
export function upsertRecord(object: string, externalIdField: string, externalId: string, values: FieldWrite[]): UpsertResult {
    const name = normalizeName(object, "object");
    const field = normalizeName(externalIdField, "externalIdField");
    const key = requireText(externalId, "externalId");
    if (key === "." || key === "..") throw validationError("invalid_input", "externalId cannot be . or ..");
    const given: FieldWrite[] = [];
    for (const write of values) {
        const { field, value } = write;
        given.push({ field: field, value: value });
    }
    const prepared = prepareValues(given, "values", true);
    if (prepared.keys.includes(field.key)) throw validationError("invalid_input", "values cannot set the external id field");
    check("salesforce.com/records.upsert", { object: name.key, externalIdField: field.key, externalId: key, fields: prepared.keys });
    const response = sfPatch("/sobjects/" + name.name + "/" + field.name + "/" + encodeComponent(key), prepared.body);
    if (response.body.trim().length === 0) {
        // An update answered with no body names no id, so read it back.
        return { id: lookupByExternalId(name.name, field.name, key), created: false };
    }
    const result = response.json<ApiSaveResult>();
    return { id: result.id ?? "", created: result.created === true || response.status === 201 };
}

/**
 * Create up to 200 records of one object in one request, all or none.
 * @param object Object API name.
 * @param records Each record's values.
 * @returns Each record's outcome, in order.
 * @capability salesforce.com/records.createMany { object: string, fields: string[], count: number, ownerIds: string[], accountIds: string[] }
 */
export function createRecords(object: string, records: FieldWrite[][]): BatchOutcome[] {
    const name = normalizeName(object, "object");
    const copies: FieldWrite[][] = [];
    for (const record of records) {
        const copy: FieldWrite[] = [];
        for (const write of record) {
            const { field, value } = write;
            copy.push({ field: field, value: value });
        }
        copies.push(copy);
    }
    batchSize(copies.length);
    const keys: string[] = [];
    const owners: string[] = [];
    const accounts: string[] = [];
    const body: Record<string, unknown>[] = [];
    for (const values of copies) {
        const prepared = prepareValues(values, "records", false);
        if (prepared.keys.length === 0) throw validationError("invalid_input", "a record's values cannot be empty");
        for (const key of prepared.keys) keys.push(key);
        owners.push(idAmong(values, "ownerid", "records"));
        accounts.push(name.key === "account" ? "" : idAmong(values, "accountid", "records"));
        body.push(withType(name.name, prepared.body));
    }
    check("salesforce.com/records.createMany", {
        object: name.key, fields: sortedDistinct(keys), count: copies.length,
        ownerIds: sortedDistinct(owners), accountIds: sortedDistinct(accounts),
    });
    return batchFrom(sfPost("/composite/sobjects", { allOrNone: true, records: body }).json<ApiSaveResult[]>());
}

/**
 * Update up to 200 records of one object in one request, all or none.
 * `OwnerId` is refused.
 * @param object Object API name.
 * @param updates Each record's id and values.
 * @returns Each record's outcome, in order.
 * @capability salesforce.com/records.updateMany { object: string, fields: string[], count: number, ownerIds: string[], accountIds: string[] }
 */
export function updateRecords(object: string, updates: RecordUpdate[]): BatchOutcome[] {
    const name = normalizeName(object, "object");
    const givenIds: string[] = [];
    const givenValues: FieldWrite[][] = [];
    for (const update of updates) {
        const { id, values } = update;
        const copy: FieldWrite[] = [];
        for (const write of values) {
            const { field, value } = write;
            copy.push({ field: field, value: value });
        }
        givenIds.push(id);
        givenValues.push(copy);
    }
    batchSize(givenIds.length);
    const keys: string[] = [];
    const ids: string[] = [];
    const body: Record<string, unknown>[] = [];
    for (let i = 0; i < givenIds.length; i += 1) {
        const id = normalizeId(givenIds[i], "updates id");
        if (ids.includes(id)) throw validationError("invalid_input", "updates repeats " + id);
        const prepared = prepareValues(givenValues[i], "updates values", true);
        if (prepared.keys.length === 0) throw validationError("invalid_input", "a record's values cannot be empty");
        for (const key of prepared.keys) keys.push(key);
        ids.push(id);
        const record = withType(name.name, prepared.body);
        record["Id"] = id;
        body.push(record);
    }
    const owners: string[] = [];
    const accounts: string[] = [];
    const quoted: string[] = [];
    for (const id of ids) quoted.push("'" + id + "'");
    const found = queryGet("SELECT FIELDS(STANDARD) FROM " + name.name + " WHERE Id IN (" + quoted.join(", ") + ")", 200);
    for (const raw of found.records) {
        owners.push(ownerOf(raw));
        accounts.push(accountOf(name.key, textField(raw, "Id"), raw));
    }
    check("salesforce.com/records.updateMany", {
        object: name.key, fields: sortedDistinct(keys), count: givenIds.length,
        ownerIds: sortedDistinct(owners), accountIds: sortedDistinct(accounts),
    });
    return batchFrom(sfPatch("/composite/sobjects", { allOrNone: true, records: body }).json<ApiSaveResult[]>());
}

/**
 * Read a record's related records through a relationship, such as an
 * account's `Contacts` or `Opportunities`, or a lookup such as a contact's `Account`.
 * @param object Object API name of the record.
 * @param id Record id.
 * @param relationship Relationship name, such as `Contacts`.
 * @param options `fields` of the related records, and the page.
 * @returns One page of related records (a lookup gives at most one).
 * @capability salesforce.com/records.related { object: string, relationship: string, ownerId: string, accountId: string }
 */
export function getRelated(object: string, id: string, relationship: string, options: RelatedOptions = {}): Page<SalesforceRecord> {
    const name = normalizeName(object, "object");
    const recordId = normalizeId(id, "id");
    const related = normalizeName(relationship, "relationship");
    const { fields: requested, pageSize, cursor } = options;
    const fields: string[] = [];
    for (const field of requested ?? []) fields.push(normalizeName(field, "fields").name);
    const window = pageWindow({ pageSize: pageSize, cursor: cursor });
    const raw = fetchRecord(name.name, recordId);
    check("salesforce.com/records.related", {
        object: name.key, relationship: related.key, ownerId: ownerOf(raw), accountId: accountOf(name.key, recordId, raw),
    });
    if (raw === null) return { records: [], totalSize: 0, done: true, nextCursor: null };
    if (window.cursor !== null) return pageFrom(cursorGet(window.cursor, window.pageSize), recordFrom);
    let path = "/sobjects/" + name.name + "/" + recordId + "/" + related.name;
    if (fields.length > 0) path += "?fields=" + encodeComponent(fields.join(","));
    const response = sfGetNullable(path, window.pageSize);
    if (response === null) return { records: [], totalSize: 0, done: true, nextCursor: null };
    const body = response.json<Record<string, unknown>>();
    if (Array.isArray(body["records"])) return pageFrom(response.json<ApiQueryResult>(), recordFrom);
    return { records: [recordFrom(body)], totalSize: 1, done: true, nextCursor: null };
}

/**
 * List the ids of an object's records created or updated in a window of at
 * most 30 days, within the last 30 days.
 * @param object Object API name.
 * @param start Window start, a date-time with a zone.
 * @param end Window end.
 * @returns The ids and the end of the window covered.
 * @capability salesforce.com/records.updated { object: string }
 */
export function getUpdated(object: string, start: string, end: string): UpdatedRecords {
    const name = normalizeName(object, "object");
    const window = changeWindow(start, end);
    check("salesforce.com/records.updated", { object: name.key });
    const result = sfGet("/sobjects/" + name.name + "/updated/" + window, 0).json<ApiUpdated>();
    return { ids: result.ids, latestDateCovered: result.latestDateCovered };
}

/**
 * List an object's records deleted in a window of at most 30 days, within the
 * last 15 days.
 * @param object Object API name.
 * @param start Window start, a date-time with a zone.
 * @param end Window end.
 * @returns The deleted records and the window covered.
 * @capability salesforce.com/records.deleted { object: string }
 */
export function getDeleted(object: string, start: string, end: string): DeletedRecords {
    const name = normalizeName(object, "object");
    const window = changeWindow(start, end);
    check("salesforce.com/records.deleted", { object: name.key });
    const result = sfGet("/sobjects/" + name.name + "/deleted/" + window, 0).json<ApiDeleted>();
    const records: DeletedRecord[] = [];
    for (const record of result.deletedRecords) records.push({ id: record.id, deletedDate: record.deletedDate });
    return { records: records, earliestDateAvailable: result.earliestDateAvailable, latestDateCovered: result.latestDateCovered };
}

// ---------------------------------------------------------------------------
// Typed standard objects
// ---------------------------------------------------------------------------

/**
 * Find accounts, most recently changed first.
 * @param filters Every filter given must match.
 * @param options Extra `fields` and the page.
 * @returns One page of accounts.
 * @capability salesforce.com/accounts.find { ownerId: string }
 */
export function findAccounts(filters: AccountFilters = {}, options: FindOptions = {}): Page<Account> {
    const where = new Filters();
    where.contains("Name", filters.name, "name");
    const ownerId = where.id("OwnerId", filters.ownerId, "ownerId");
    where.text("Industry", filters.industry, "industry");
    where.text("Type", filters.type, "type");
    where.dateTime("LastModifiedDate", ">", filters.modifiedAfter, "modifiedAfter");
    const { fields, pageSize, cursor } = options;
    const requested: string[] = [];
    for (const field of fields ?? []) requested.push(field);
    const extra = extraFields(requested);
    const window = pageWindow({ pageSize: pageSize, cursor: cursor });
    check("salesforce.com/accounts.find", { ownerId: ownerId });
    return findTyped("Account", where.conditions, extra, window, accountFrom);
}

/**
 * Get an account; null when it does not exist.
 * @param id Account id.
 * @param options Extra `fields`.
 * @returns The account, or null.
 * @capability salesforce.com/accounts.get { ownerId: string, accountId: string }
 */
export function getAccount(id: string, options: ReadOptions = {}): Account | null {
    const recordId = normalizeId(id, "id");
    const requested: string[] = [];
    for (const field of options.fields ?? []) requested.push(field);
    const extra = extraFields(requested);
    const fields = readOne("Account", recordId, extra);
    const account = fields === null ? null : accountFrom(fields, extra.requested);
    check("salesforce.com/accounts.get", { ownerId: account === null ? "" : account.ownerId ?? "", accountId: recordId });
    return account;
}

/**
 * Find contacts, most recently changed first.
 * @param filters Every filter given must match.
 * @param options Extra `fields` and the page.
 * @returns One page of contacts.
 * @capability salesforce.com/contacts.find { accountId: string }
 */
export function findContacts(filters: ContactFilters = {}, options: FindOptions = {}): Page<Contact> {
    const where = new Filters();
    const accountId = where.id("AccountId", filters.accountId, "accountId");
    where.text("Email", filters.email, "email");
    where.contains("Name", filters.name, "name");
    const { fields, pageSize, cursor } = options;
    const requested: string[] = [];
    for (const field of fields ?? []) requested.push(field);
    const extra = extraFields(requested);
    const window = pageWindow({ pageSize: pageSize, cursor: cursor });
    check("salesforce.com/contacts.find", { accountId: accountId });
    return findTyped("Contact", where.conditions, extra, window, contactFrom);
}

/**
 * Get a contact; null when it does not exist.
 * @param id Contact id.
 * @param options Extra `fields`.
 * @returns The contact, or null.
 * @capability salesforce.com/contacts.get { ownerId: string, accountId: string }
 */
export function getContact(id: string, options: ReadOptions = {}): Contact | null {
    const recordId = normalizeId(id, "id");
    const requested: string[] = [];
    for (const field of options.fields ?? []) requested.push(field);
    const extra = extraFields(requested);
    const fields = readOne("Contact", recordId, extra);
    const contact = fields === null ? null : contactFrom(fields, extra.requested);
    check("salesforce.com/contacts.get", {
        ownerId: contact === null ? "" : contact.ownerId ?? "", accountId: contact === null ? "" : contact.accountId ?? "",
    });
    return contact;
}

/**
 * Create a contact.
 * @param input The contact's fields; `custom` sets others by API name.
 * @returns The new contact's id.
 * @capability salesforce.com/contacts.create { accountId: string, ownerId: string, fields: string[] }
 */
export function createContact(input: ContactInput): string {
    const {
        lastName, firstName, accountId: account, email, phone, mobilePhone, title, department, leadSource, description,
        ownerId: owner, custom,
    } = input;
    const extra: FieldWrite[] = [];
    for (const write of custom ?? []) {
        const { field, value } = write;
        extra.push({ field: field, value: value });
    }
    const accountId = account === undefined ? "" : normalizeId(account, "accountId");
    const ownerId = owner === undefined ? "" : normalizeId(owner, "ownerId");
    const writes = new Writes();
    writes.set("LastName", requireText(lastName, "lastName"));
    writes.set("FirstName", firstName);
    if (accountId.length > 0) writes.set("AccountId", accountId);
    writes.set("Email", email);
    writes.set("Phone", phone);
    writes.set("MobilePhone", mobilePhone);
    writes.set("Title", title);
    writes.set("Department", department);
    writes.set("LeadSource", leadSource);
    writes.set("Description", description);
    if (ownerId.length > 0) writes.set("OwnerId", ownerId);
    const prepared = writes.prepare(extra, "input");
    check("salesforce.com/contacts.create", { accountId: accountId, ownerId: ownerId, fields: prepared.keys });
    return createdId(sfPost("/sobjects/Contact", prepared.body));
}

/**
 * Find leads, most recently changed first.
 * @param filters Every filter given must match.
 * @param options Extra `fields` and the page.
 * @returns One page of leads.
 * @capability salesforce.com/leads.find { ownerId: string }
 */
export function findLeads(filters: LeadFilters = {}, options: FindOptions = {}): Page<Lead> {
    const where = new Filters();
    const ownerId = where.id("OwnerId", filters.ownerId, "ownerId");
    where.text("Status", filters.status, "status");
    where.text("Email", filters.email, "email");
    where.contains("Company", filters.company, "company");
    where.flag("IsConverted", filters.isConverted);
    const { fields, pageSize, cursor } = options;
    const requested: string[] = [];
    for (const field of fields ?? []) requested.push(field);
    const extra = extraFields(requested);
    const window = pageWindow({ pageSize: pageSize, cursor: cursor });
    check("salesforce.com/leads.find", { ownerId: ownerId });
    return findTyped("Lead", where.conditions, extra, window, leadFrom);
}

/**
 * Get a lead; null when it does not exist.
 * @param id Lead id.
 * @param options Extra `fields`.
 * @returns The lead, or null.
 * @capability salesforce.com/leads.get { ownerId: string }
 */
export function getLead(id: string, options: ReadOptions = {}): Lead | null {
    const recordId = normalizeId(id, "id");
    const requested: string[] = [];
    for (const field of options.fields ?? []) requested.push(field);
    const extra = extraFields(requested);
    const fields = readOne("Lead", recordId, extra);
    const lead = fields === null ? null : leadFrom(fields, extra.requested);
    check("salesforce.com/leads.get", { ownerId: lead === null ? "" : lead.ownerId ?? "" });
    return lead;
}

/**
 * Create a lead.
 * @param input The lead's fields; `custom` sets others by API name.
 * @returns The new lead's id.
 * @capability salesforce.com/leads.create { ownerId: string, status: string, fields: string[] }
 */
export function createLead(input: LeadInput): string {
    const {
        lastName, company, firstName, email, phone, title, status: givenStatus, leadSource, industry, rating, description,
        ownerId: owner, custom,
    } = input;
    const extra: FieldWrite[] = [];
    for (const write of custom ?? []) {
        const { field, value } = write;
        extra.push({ field: field, value: value });
    }
    const ownerId = owner === undefined ? "" : normalizeId(owner, "ownerId");
    const status = givenStatus === undefined ? "" : requireText(givenStatus, "status");
    const writes = new Writes();
    writes.set("LastName", requireText(lastName, "lastName"));
    writes.set("Company", requireText(company, "company"));
    writes.set("FirstName", firstName);
    writes.set("Email", email);
    writes.set("Phone", phone);
    writes.set("Title", title);
    if (status.length > 0) writes.set("Status", status);
    writes.set("LeadSource", leadSource);
    writes.set("Industry", industry);
    writes.set("Rating", rating);
    writes.set("Description", description);
    if (ownerId.length > 0) writes.set("OwnerId", ownerId);
    const prepared = writes.prepare(extra, "input");
    check("salesforce.com/leads.create", { ownerId: ownerId, status: status, fields: prepared.keys });
    return createdId(sfPost("/sobjects/Lead", prepared.body));
}

/**
 * Update a lead. A field left out keeps its value; `null` clears it.
 * @param id Lead id.
 * @param changes The fields to change.
 * @capability salesforce.com/leads.update { ownerId: string, status: string, previousStatus: string, fields: string[] }
 */
export function updateLead(id: string, changes: LeadUpdate): void {
    const recordId = normalizeId(id, "id");
    const {
        firstName, lastName, company, email, phone, title, status: givenStatus, leadSource, industry, rating, description, custom,
    } = changes;
    const extra: FieldWrite[] = [];
    for (const write of custom ?? []) {
        const { field, value } = write;
        extra.push({ field: field, value: value });
    }
    const status = givenStatus === undefined ? "" : requireText(givenStatus, "status");
    const writes = new Writes();
    writes.set("FirstName", firstName);
    writes.set("LastName", lastName === undefined ? undefined : requireText(lastName, "lastName"));
    writes.set("Company", company === undefined ? undefined : requireText(company, "company"));
    writes.set("Email", email);
    writes.set("Phone", phone);
    writes.set("Title", title);
    if (status.length > 0) writes.set("Status", status);
    writes.set("LeadSource", leadSource);
    writes.set("Industry", industry);
    writes.set("Rating", rating);
    writes.set("Description", description);
    const prepared = writes.prepare(extra, "changes");
    const current = lookup("SELECT Id, OwnerId, Status FROM Lead", recordId);
    check("salesforce.com/leads.update", {
        ownerId: ownerOf(current), status: status, previousStatus: textOf(current, "Status"), fields: prepared.keys,
    });
    sfPatch("/sobjects/Lead/" + recordId, prepared.body);
}

/**
 * Find opportunities, most recently changed first.
 * @param filters Every filter given must match.
 * @param options Extra `fields` and the page.
 * @returns One page of opportunities.
 * @capability salesforce.com/opportunities.find { accountId: string, ownerId: string }
 */
export function findOpportunities(filters: OpportunityFilters = {}, options: FindOptions = {}): Page<Opportunity> {
    const where = new Filters();
    const accountId = where.id("AccountId", filters.accountId, "accountId");
    const ownerId = where.id("OwnerId", filters.ownerId, "ownerId");
    where.text("StageName", filters.stage, "stage");
    where.flag("IsClosed", filters.isClosed);
    where.flag("IsWon", filters.isWon);
    where.date("CloseDate", ">=", filters.closeAfter, "closeAfter");
    where.date("CloseDate", "<=", filters.closeBefore, "closeBefore");
    where.atLeast("Amount", filters.minAmount, "minAmount");
    const { fields, pageSize, cursor } = options;
    const requested: string[] = [];
    for (const field of fields ?? []) requested.push(field);
    const extra = extraFields(requested);
    const window = pageWindow({ pageSize: pageSize, cursor: cursor });
    check("salesforce.com/opportunities.find", { accountId: accountId, ownerId: ownerId });
    return findTyped("Opportunity", where.conditions, extra, window, opportunityFrom);
}

/**
 * Get an opportunity; null when it does not exist.
 * @param id Opportunity id.
 * @param options Extra `fields`.
 * @returns The opportunity, or null.
 * @capability salesforce.com/opportunities.get { ownerId: string, accountId: string }
 */
export function getOpportunity(id: string, options: ReadOptions = {}): Opportunity | null {
    const recordId = normalizeId(id, "id");
    const requested: string[] = [];
    for (const field of options.fields ?? []) requested.push(field);
    const extra = extraFields(requested);
    const fields = readOne("Opportunity", recordId, extra);
    const opportunity = fields === null ? null : opportunityFrom(fields, extra.requested);
    check("salesforce.com/opportunities.get", {
        ownerId: opportunity === null ? "" : opportunity.ownerId ?? "",
        accountId: opportunity === null ? "" : opportunity.accountId ?? "",
    });
    return opportunity;
}

/**
 * Update an opportunity, such as its stage, close date or amount. A field left
 * out keeps its value; `null` clears it.
 * @param id Opportunity id.
 * @param changes The fields to change.
 * @capability salesforce.com/opportunities.update { accountId: string, ownerId: string, stage: string, previousStage: string, fields: string[] }
 */
export function updateOpportunity(id: string, changes: OpportunityUpdate): void {
    const recordId = normalizeId(id, "id");
    const { name, stage: givenStage, closeDate, amount, probability, nextStep, type, leadSource, description, custom } = changes;
    const extra: FieldWrite[] = [];
    for (const write of custom ?? []) {
        const { field, value } = write;
        extra.push({ field: field, value: value });
    }
    const stage = givenStage === undefined ? "" : requireText(givenStage, "stage");
    const writes = new Writes();
    writes.set("Name", name === undefined ? undefined : requireText(name, "name"));
    if (stage.length > 0) writes.set("StageName", stage);
    writes.setDate("CloseDate", closeDate, "closeDate");
    writes.set("Amount", amount);
    writes.set("Probability", probability);
    writes.set("NextStep", nextStep);
    writes.set("Type", type);
    writes.set("LeadSource", leadSource);
    writes.set("Description", description);
    const prepared = writes.prepare(extra, "changes");
    const current = lookup("SELECT Id, OwnerId, AccountId, StageName FROM Opportunity", recordId);
    check("salesforce.com/opportunities.update", {
        accountId: textOf(current, "AccountId"), ownerId: ownerOf(current), stage: stage,
        previousStage: textOf(current, "StageName"), fields: prepared.keys,
    });
    sfPatch("/sobjects/Opportunity/" + recordId, prepared.body);
}

/**
 * Find an opportunity's products.
 * @param filters The opportunity.
 * @param options Extra `fields` and the page.
 * @returns One page of line items.
 * @capability salesforce.com/opportunityLineItems.find { opportunityId: string }
 */
export function findOpportunityLineItems(filters: OpportunityLineItemFilters, options: FindOptions = {}): Page<OpportunityLineItem> {
    const where = new Filters();
    const opportunityId = where.id("OpportunityId", filters.opportunityId, "opportunityId");
    const { fields, pageSize, cursor } = options;
    const requested: string[] = [];
    for (const field of fields ?? []) requested.push(field);
    const extra = extraFields(requested);
    const window = pageWindow({ pageSize: pageSize, cursor: cursor });
    check("salesforce.com/opportunityLineItems.find", { opportunityId: opportunityId });
    return findTyped("OpportunityLineItem", where.conditions, extra, window, lineItemFrom);
}

/**
 * Get an opportunity's product; null when it does not exist.
 * @param id Record id.
 * @param options Extra `fields`.
 * @returns The record, or null.
 * @capability salesforce.com/opportunityLineItems.get { opportunityId: string }
 */
export function getOpportunityLineItem(id: string, options: ReadOptions = {}): OpportunityLineItem | null {
    const recordId = normalizeId(id, "id");
    const requested: string[] = [];
    for (const field of options.fields ?? []) requested.push(field);
    const extra = extraFields(requested);
    const fields = readOne("OpportunityLineItem", recordId, extra);
    const found = fields === null ? null : lineItemFrom(fields, extra.requested);
    check("salesforce.com/opportunityLineItems.get", { opportunityId: found === null ? "" : found.opportunityId ?? "" });
    return found;
}

/**
 * Find products.
 * @param filters Every filter given must match.
 * @param options Extra `fields` and the page.
 * @returns One page of products.
 * @capability salesforce.com/products.find {}
 */
export function findProducts(filters: ProductFilters = {}, options: FindOptions = {}): Page<Product> {
    const where = new Filters();
    where.contains("Name", filters.name, "name");
    where.text("Family", filters.family, "family");
    where.flag("IsActive", filters.isActive);
    const { fields, pageSize, cursor } = options;
    const requested: string[] = [];
    for (const field of fields ?? []) requested.push(field);
    const extra = extraFields(requested);
    const window = pageWindow({ pageSize: pageSize, cursor: cursor });
    check("salesforce.com/products.find", {});
    return findTyped("Product2", where.conditions, extra, window, productFrom);
}

/**
 * Get a product; null when it does not exist.
 * @param id Record id.
 * @param options Extra `fields`.
 * @returns The record, or null.
 * @capability salesforce.com/products.get {}
 */
export function getProduct(id: string, options: ReadOptions = {}): Product | null {
    const recordId = normalizeId(id, "id");
    const requested: string[] = [];
    for (const field of options.fields ?? []) requested.push(field);
    const extra = extraFields(requested);
    const fields = readOne("Product2", recordId, extra);
    const found = fields === null ? null : productFrom(fields, extra.requested);
    check("salesforce.com/products.get", {});
    return found;
}

/**
 * Find price books.
 * @param filters Every filter given must match.
 * @param options Extra `fields` and the page.
 * @returns One page of price books.
 * @capability salesforce.com/pricebooks.find {}
 */
export function findPricebooks(filters: PricebookFilters = {}, options: FindOptions = {}): Page<Pricebook> {
    const where = new Filters();
    where.flag("IsActive", filters.isActive);
    where.flag("IsStandard", filters.isStandard);
    const { fields, pageSize, cursor } = options;
    const requested: string[] = [];
    for (const field of fields ?? []) requested.push(field);
    const extra = extraFields(requested);
    const window = pageWindow({ pageSize: pageSize, cursor: cursor });
    check("salesforce.com/pricebooks.find", {});
    return findTyped("Pricebook2", where.conditions, extra, window, pricebookFrom);
}

/**
 * Get a price book; null when it does not exist.
 * @param id Record id.
 * @param options Extra `fields`.
 * @returns The record, or null.
 * @capability salesforce.com/pricebooks.get {}
 */
export function getPricebook(id: string, options: ReadOptions = {}): Pricebook | null {
    const recordId = normalizeId(id, "id");
    const requested: string[] = [];
    for (const field of options.fields ?? []) requested.push(field);
    const extra = extraFields(requested);
    const fields = readOne("Pricebook2", recordId, extra);
    const found = fields === null ? null : pricebookFrom(fields, extra.requested);
    check("salesforce.com/pricebooks.get", {});
    return found;
}

/**
 * Find cases, most recently changed first.
 * @param filters Every filter given must match.
 * @param options Extra `fields` and the page.
 * @returns One page of cases.
 * @capability salesforce.com/cases.find { accountId: string, ownerId: string }
 */
export function findCases(filters: CaseFilters = {}, options: FindOptions = {}): Page<Case> {
    const where = new Filters();
    const accountId = where.id("AccountId", filters.accountId, "accountId");
    const ownerId = where.id("OwnerId", filters.ownerId, "ownerId");
    where.id("ContactId", filters.contactId, "contactId");
    where.text("Status", filters.status, "status");
    where.text("Priority", filters.priority, "priority");
    where.flag("IsClosed", filters.isClosed);
    where.dateTime("CreatedDate", ">", filters.createdAfter, "createdAfter");
    const { fields, pageSize, cursor } = options;
    const requested: string[] = [];
    for (const field of fields ?? []) requested.push(field);
    const extra = extraFields(requested);
    const window = pageWindow({ pageSize: pageSize, cursor: cursor });
    check("salesforce.com/cases.find", { accountId: accountId, ownerId: ownerId });
    return findTyped("Case", where.conditions, extra, window, caseFrom);
}

/**
 * Get a case; null when it does not exist.
 * @param id Case id.
 * @param options Extra `fields`.
 * @returns The case, or null.
 * @capability salesforce.com/cases.get { ownerId: string, accountId: string }
 */
export function getCase(id: string, options: ReadOptions = {}): Case | null {
    const recordId = normalizeId(id, "id");
    const requested: string[] = [];
    for (const field of options.fields ?? []) requested.push(field);
    const extra = extraFields(requested);
    const fields = readOne("Case", recordId, extra);
    const found = fields === null ? null : caseFrom(fields, extra.requested);
    check("salesforce.com/cases.get", {
        ownerId: found === null ? "" : found.ownerId ?? "", accountId: found === null ? "" : found.accountId ?? "",
    });
    return found;
}

/**
 * Create a case.
 * @param input The case's fields; `custom` sets others by API name.
 * @returns The new case's id.
 * @capability salesforce.com/cases.create { accountId: string, ownerId: string, fields: string[] }
 */
export function createCase(input: CaseInput): string {
    const {
        subject, description, status, priority, origin, type, reason, accountId: account, contactId, ownerId: owner, custom,
    } = input;
    const extra: FieldWrite[] = [];
    for (const write of custom ?? []) {
        const { field, value } = write;
        extra.push({ field: field, value: value });
    }
    const accountId = account === undefined ? "" : normalizeId(account, "accountId");
    const ownerId = owner === undefined ? "" : normalizeId(owner, "ownerId");
    const writes = new Writes();
    writes.set("Subject", requireText(subject, "subject"));
    writes.set("Description", description);
    writes.set("Status", status);
    writes.set("Priority", priority);
    writes.set("Origin", origin);
    writes.set("Type", type);
    writes.set("Reason", reason);
    if (accountId.length > 0) writes.set("AccountId", accountId);
    writes.setId("ContactId", contactId, "contactId");
    if (ownerId.length > 0) writes.set("OwnerId", ownerId);
    const prepared = writes.prepare(extra, "input");
    check("salesforce.com/cases.create", { accountId: accountId, ownerId: ownerId, fields: prepared.keys });
    return createdId(sfPost("/sobjects/Case", prepared.body));
}

/**
 * Update a case. A field left out keeps its value; `null` clears it.
 * @param id Case id.
 * @param changes The fields to change.
 * @capability salesforce.com/cases.update { accountId: string, ownerId: string, status: string, previousStatus: string, fields: string[] }
 */
export function updateCase(id: string, changes: CaseUpdate): void {
    const recordId = normalizeId(id, "id");
    const { subject, description, status: givenStatus, priority, origin, type, reason, contactId, custom } = changes;
    const extra: FieldWrite[] = [];
    for (const write of custom ?? []) {
        const { field, value } = write;
        extra.push({ field: field, value: value });
    }
    const status = givenStatus === undefined ? "" : requireText(givenStatus, "status");
    const writes = new Writes();
    writes.set("Subject", subject);
    writes.set("Description", description);
    if (status.length > 0) writes.set("Status", status);
    writes.set("Priority", priority);
    writes.set("Origin", origin);
    writes.set("Type", type);
    writes.set("Reason", reason);
    writes.setId("ContactId", contactId, "contactId");
    const prepared = writes.prepare(extra, "changes");
    const current = lookup("SELECT Id, OwnerId, AccountId, Status FROM Case", recordId);
    check("salesforce.com/cases.update", {
        accountId: textOf(current, "AccountId"), ownerId: ownerOf(current), status: status,
        previousStatus: textOf(current, "Status"), fields: prepared.keys,
    });
    sfPatch("/sobjects/Case/" + recordId, prepared.body);
}

/**
 * Find tasks, most recently changed first.
 * @param filters Every filter given must match.
 * @param options Extra `fields` and the page.
 * @returns One page of tasks.
 * @capability salesforce.com/tasks.find { ownerId: string, whatId: string }
 */
export function findTasks(filters: TaskFilters = {}, options: FindOptions = {}): Page<Task> {
    const where = new Filters();
    const whatId = where.id("WhatId", filters.whatId, "whatId");
    where.id("WhoId", filters.whoId, "whoId");
    const ownerId = where.id("OwnerId", filters.ownerId, "ownerId");
    where.text("Status", filters.status, "status");
    where.flag("IsClosed", filters.isClosed);
    where.date("ActivityDate", ">=", filters.dueAfter, "dueAfter");
    where.date("ActivityDate", "<=", filters.dueBefore, "dueBefore");
    const { fields, pageSize, cursor } = options;
    const requested: string[] = [];
    for (const field of fields ?? []) requested.push(field);
    const extra = extraFields(requested);
    const window = pageWindow({ pageSize: pageSize, cursor: cursor });
    check("salesforce.com/tasks.find", { ownerId: ownerId, whatId: whatId });
    return findTyped("Task", where.conditions, extra, window, taskFrom);
}

/**
 * Get a task; null when it does not exist.
 * @param id Record id.
 * @param options Extra `fields`.
 * @returns The record, or null.
 * @capability salesforce.com/tasks.get { ownerId: string, accountId: string }
 */
export function getTask(id: string, options: ReadOptions = {}): Task | null {
    const recordId = normalizeId(id, "id");
    const requested: string[] = [];
    for (const field of options.fields ?? []) requested.push(field);
    const extra = extraFields(requested);
    const fields = readOne("Task", recordId, extra);
    const found = fields === null ? null : taskFrom(fields, extra.requested);
    check("salesforce.com/tasks.get", { ownerId: found === null ? "" : found.ownerId ?? "", accountId: found === null ? "" : found.accountId ?? "" });
    return found;
}

/**
 * Find events, most recently changed first.
 * @param filters Every filter given must match.
 * @param options Extra `fields` and the page.
 * @returns One page of events.
 * @capability salesforce.com/events.find { ownerId: string, whatId: string }
 */
export function findEvents(filters: EventFilters = {}, options: FindOptions = {}): Page<Event> {
    const where = new Filters();
    const whatId = where.id("WhatId", filters.whatId, "whatId");
    where.id("WhoId", filters.whoId, "whoId");
    const ownerId = where.id("OwnerId", filters.ownerId, "ownerId");
    where.dateTime("StartDateTime", ">=", filters.startAfter, "startAfter");
    where.dateTime("StartDateTime", "<=", filters.startBefore, "startBefore");
    const { fields, pageSize, cursor } = options;
    const requested: string[] = [];
    for (const field of fields ?? []) requested.push(field);
    const extra = extraFields(requested);
    const window = pageWindow({ pageSize: pageSize, cursor: cursor });
    check("salesforce.com/events.find", { ownerId: ownerId, whatId: whatId });
    return findTyped("Event", where.conditions, extra, window, eventFrom);
}

/**
 * Get an event; null when it does not exist.
 * @param id Record id.
 * @param options Extra `fields`.
 * @returns The record, or null.
 * @capability salesforce.com/events.get { ownerId: string, accountId: string }
 */
export function getEvent(id: string, options: ReadOptions = {}): Event | null {
    const recordId = normalizeId(id, "id");
    const requested: string[] = [];
    for (const field of options.fields ?? []) requested.push(field);
    const extra = extraFields(requested);
    const fields = readOne("Event", recordId, extra);
    const found = fields === null ? null : eventFrom(fields, extra.requested);
    check("salesforce.com/events.get", { ownerId: found === null ? "" : found.ownerId ?? "", accountId: found === null ? "" : found.accountId ?? "" });
    return found;
}

/**
 * Find users of the org.
 * @param filters Every filter given must match.
 * @param options Extra `fields` and the page.
 * @returns One page of users.
 * @capability salesforce.com/users.find {}
 */
export function findUsers(filters: UserFilters = {}, options: FindOptions = {}): Page<User> {
    const where = new Filters();
    where.contains("Name", filters.name, "name");
    where.text("Email", filters.email, "email");
    where.flag("IsActive", filters.isActive);
    const { fields, pageSize, cursor } = options;
    const requested: string[] = [];
    for (const field of fields ?? []) requested.push(field);
    const extra = extraFields(requested);
    const window = pageWindow({ pageSize: pageSize, cursor: cursor });
    check("salesforce.com/users.find", {});
    return findTyped("User", where.conditions, extra, window, userFrom);
}

/**
 * Get a user; null when it does not exist.
 * @param id Record id.
 * @param options Extra `fields`.
 * @returns The record, or null.
 * @capability salesforce.com/users.get {}
 */
export function getUser(id: string, options: ReadOptions = {}): User | null {
    const recordId = normalizeId(id, "id");
    const requested: string[] = [];
    for (const field of options.fields ?? []) requested.push(field);
    const extra = extraFields(requested);
    const fields = readOne("User", recordId, extra);
    const found = fields === null ? null : userFrom(fields, extra.requested);
    check("salesforce.com/users.get", {});
    return found;
}

/**
 * Find campaigns, most recently changed first.
 * @param filters Every filter given must match.
 * @param options Extra `fields` and the page.
 * @returns One page of campaigns.
 * @capability salesforce.com/campaigns.find { ownerId: string }
 */
export function findCampaigns(filters: CampaignFilters = {}, options: FindOptions = {}): Page<Campaign> {
    const where = new Filters();
    where.contains("Name", filters.name, "name");
    const ownerId = where.id("OwnerId", filters.ownerId, "ownerId");
    where.text("Status", filters.status, "status");
    where.text("Type", filters.type, "type");
    where.flag("IsActive", filters.isActive);
    const { fields, pageSize, cursor } = options;
    const requested: string[] = [];
    for (const field of fields ?? []) requested.push(field);
    const extra = extraFields(requested);
    const window = pageWindow({ pageSize: pageSize, cursor: cursor });
    check("salesforce.com/campaigns.find", { ownerId: ownerId });
    return findTyped("Campaign", where.conditions, extra, window, campaignFrom);
}

/**
 * Get a campaign; null when it does not exist.
 * @param id Record id.
 * @param options Extra `fields`.
 * @returns The record, or null.
 * @capability salesforce.com/campaigns.get { ownerId: string }
 */
export function getCampaign(id: string, options: ReadOptions = {}): Campaign | null {
    const recordId = normalizeId(id, "id");
    const requested: string[] = [];
    for (const field of options.fields ?? []) requested.push(field);
    const extra = extraFields(requested);
    const fields = readOne("Campaign", recordId, extra);
    const found = fields === null ? null : campaignFrom(fields, extra.requested);
    check("salesforce.com/campaigns.get", { ownerId: found === null ? "" : found.ownerId ?? "" });
    return found;
}

/**
 * Find campaign members.
 * @param filters Every filter given must match.
 * @param options Extra `fields` and the page.
 * @returns One page of campaign members.
 * @capability salesforce.com/campaignMembers.find { campaignId: string }
 */
export function findCampaignMembers(filters: CampaignMemberFilters = {}, options: FindOptions = {}): Page<CampaignMember> {
    const where = new Filters();
    const campaignId = where.id("CampaignId", filters.campaignId, "campaignId");
    where.id("LeadId", filters.leadId, "leadId");
    where.id("ContactId", filters.contactId, "contactId");
    const { fields, pageSize, cursor } = options;
    const requested: string[] = [];
    for (const field of fields ?? []) requested.push(field);
    const extra = extraFields(requested);
    const window = pageWindow({ pageSize: pageSize, cursor: cursor });
    check("salesforce.com/campaignMembers.find", { campaignId: campaignId });
    return findTyped("CampaignMember", where.conditions, extra, window, campaignMemberFrom);
}

/**
 * Get a campaign member; null when it does not exist.
 * @param id Record id.
 * @param options Extra `fields`.
 * @returns The record, or null.
 * @capability salesforce.com/campaignMembers.get { campaignId: string }
 */
export function getCampaignMember(id: string, options: ReadOptions = {}): CampaignMember | null {
    const recordId = normalizeId(id, "id");
    const requested: string[] = [];
    for (const field of options.fields ?? []) requested.push(field);
    const extra = extraFields(requested);
    const fields = readOne("CampaignMember", recordId, extra);
    const found = fields === null ? null : campaignMemberFrom(fields, extra.requested);
    check("salesforce.com/campaignMembers.get", { campaignId: found === null ? "" : found.campaignId ?? "" });
    return found;
}

/**
 * Find contracts, most recently changed first.
 * @param filters Every filter given must match.
 * @param options Extra `fields` and the page.
 * @returns One page of contracts.
 * @capability salesforce.com/contracts.find { accountId: string, ownerId: string }
 */
export function findContracts(filters: AccountRecordFilters = {}, options: FindOptions = {}): Page<Contract> {
    const where = new Filters();
    const accountId = where.id("AccountId", filters.accountId, "accountId");
    const ownerId = where.id("OwnerId", filters.ownerId, "ownerId");
    where.text("Status", filters.status, "status");
    const { fields, pageSize, cursor } = options;
    const requested: string[] = [];
    for (const field of fields ?? []) requested.push(field);
    const extra = extraFields(requested);
    const window = pageWindow({ pageSize: pageSize, cursor: cursor });
    check("salesforce.com/contracts.find", { accountId: accountId, ownerId: ownerId });
    return findTyped("Contract", where.conditions, extra, window, contractFrom);
}

/**
 * Get a contract; null when it does not exist.
 * @param id Record id.
 * @param options Extra `fields`.
 * @returns The record, or null.
 * @capability salesforce.com/contracts.get { ownerId: string, accountId: string }
 */
export function getContract(id: string, options: ReadOptions = {}): Contract | null {
    const recordId = normalizeId(id, "id");
    const requested: string[] = [];
    for (const field of options.fields ?? []) requested.push(field);
    const extra = extraFields(requested);
    const fields = readOne("Contract", recordId, extra);
    const found = fields === null ? null : contractFrom(fields, extra.requested);
    check("salesforce.com/contracts.get", { ownerId: found === null ? "" : found.ownerId ?? "", accountId: found === null ? "" : found.accountId ?? "" });
    return found;
}

/**
 * Find orders, most recently changed first.
 * @param filters Every filter given must match.
 * @param options Extra `fields` and the page.
 * @returns One page of orders.
 * @capability salesforce.com/orders.find { accountId: string, ownerId: string }
 */
export function findOrders(filters: AccountRecordFilters = {}, options: FindOptions = {}): Page<Order> {
    const where = new Filters();
    const accountId = where.id("AccountId", filters.accountId, "accountId");
    const ownerId = where.id("OwnerId", filters.ownerId, "ownerId");
    where.text("Status", filters.status, "status");
    const { fields, pageSize, cursor } = options;
    const requested: string[] = [];
    for (const field of fields ?? []) requested.push(field);
    const extra = extraFields(requested);
    const window = pageWindow({ pageSize: pageSize, cursor: cursor });
    check("salesforce.com/orders.find", { accountId: accountId, ownerId: ownerId });
    return findTyped("Order", where.conditions, extra, window, orderFrom);
}

/**
 * Get an order; null when it does not exist.
 * @param id Record id.
 * @param options Extra `fields`.
 * @returns The record, or null.
 * @capability salesforce.com/orders.get { ownerId: string, accountId: string }
 */
export function getOrder(id: string, options: ReadOptions = {}): Order | null {
    const recordId = normalizeId(id, "id");
    const requested: string[] = [];
    for (const field of options.fields ?? []) requested.push(field);
    const extra = extraFields(requested);
    const fields = readOne("Order", recordId, extra);
    const found = fields === null ? null : orderFrom(fields, extra.requested);
    check("salesforce.com/orders.get", { ownerId: found === null ? "" : found.ownerId ?? "", accountId: found === null ? "" : found.accountId ?? "" });
    return found;
}

// ---------------------------------------------------------------------------
// Queries and search
// ---------------------------------------------------------------------------

/**
 * Query one object's records. The package writes the SOQL: names are checked
 * and every value is encoded, so `where` values never need escaping.
 * @param spec The object, fields, conditions, sort and limit.
 * @param page The page; pass the same `spec` with a `cursor`.
 * @returns One page of records.
 * @capability salesforce.com/query.run { object: string, fields: string[] }
 */
export function query(spec: QuerySpec, page: PageOptions = {}): Page<SalesforceRecord> {
    // Copied once, field by field: the checked spec is the spec sent.
    const { object, fields, where, orderBy, limit } = spec;
    const fieldsCopy: string[] = [];
    for (const field of fields) fieldsCopy.push(field);
    const whereCopy: Condition[] = [];
    for (const condition of where ?? []) {
        const { field, op, value, values, date, dateTime, dateLiteral } = condition;
        let list: (string | number)[] | undefined = undefined;
        if (values !== undefined) {
            const items: (string | number)[] = [];
            for (const item of values) items.push(item);
            list = items;
        }
        whereCopy.push({ field: field, op: op, value: value, values: list, date: date, dateTime: dateTime, dateLiteral: dateLiteral });
    }
    const orderCopy: OrderBy[] = [];
    for (const sort of orderBy ?? []) {
        const { field, direction, nulls } = sort;
        orderCopy.push({ field: field, direction: direction, nulls: nulls });
    }
    const prepared = buildQuery({ object: object, fields: fieldsCopy, where: whereCopy, orderBy: orderCopy, limit: limit });
    const { pageSize, cursor } = page;
    const window = pageWindow({ pageSize: pageSize, cursor: cursor });
    check("salesforce.com/query.run", { object: prepared.object, fields: prepared.fields });
    const result = window.cursor === null ? queryGet(prepared.soql, window.pageSize) : cursorGet(window.cursor, window.pageSize);
    return pageFrom(result, recordFrom);
}

/**
 * Count, sum, average, or find the minimum or maximum of records' fields,
 * optionally grouped, such as the pipeline's amount by stage or by close month.
 * @param spec The object, groups, measures, conditions, sort and limit.
 * @returns At most 2000 rows, keyed by the groups' and measures' labels.
 * @capability salesforce.com/query.aggregate { object: string, fields: string[] }
 */
export function aggregate(spec: AggregateSpec): AggregateResult {
    // Copied once, field by field: the checked spec is the spec sent.
    const { object, groupBy, measures, where, orderBy, limit } = spec;
    const groupCopy: GroupKey[] = [];
    for (const group of groupBy ?? []) {
        const { field, date } = group;
        groupCopy.push({ field: field, date: date });
    }
    const measureCopy: Measure[] = [];
    for (const measure of measures) {
        const { fn, field } = measure;
        measureCopy.push({ fn: fn, field: field });
    }
    const whereCopy: Condition[] = [];
    for (const condition of where ?? []) {
        const { field, op, value, values, date, dateTime, dateLiteral } = condition;
        let list: (string | number)[] | undefined = undefined;
        if (values !== undefined) {
            const items: (string | number)[] = [];
            for (const item of values) items.push(item);
            list = items;
        }
        whereCopy.push({ field: field, op: op, value: value, values: list, date: date, dateTime: dateTime, dateLiteral: dateLiteral });
    }
    const orderCopy: AggregateOrder[] = [];
    for (const sort of orderBy ?? []) {
        const { by, direction } = sort;
        orderCopy.push({ by: by, direction: direction });
    }
    const prepared = buildAggregate({
        object: object, groupBy: groupCopy, measures: measureCopy, where: whereCopy, orderBy: orderCopy, limit: limit,
    });
    check("salesforce.com/query.aggregate", { object: prepared.object, fields: prepared.fields });
    return aggregateFrom(queryGet(prepared.soql, 2000), prepared.groups, prepared.measures);
}

/**
 * Search text across objects, such as a name, email or phone number in
 * accounts, contacts and leads.
 * @param spec The text, the objects and their fields, and limits.
 * @returns The matching records, best matches first.
 * @capability salesforce.com/query.search { objects: string[], fields: string[] }
 */
export function search(spec: SearchSpec): SalesforceRecord[] {
    // Copied once, field by field: the checked spec is the spec sent.
    const { text, objects, limit } = spec;
    const objectCopy: SearchObject[] = [];
    for (const target of objects) {
        const { object, fields, limit: objectLimit } = target;
        let list: string[] | undefined = undefined;
        if (fields !== undefined) {
            const items: string[] = [];
            for (const field of fields) items.push(field);
            list = items;
        }
        objectCopy.push({ object: object, fields: list, limit: objectLimit });
    }
    const prepared = buildSearch({ text: text, objects: objectCopy, limit: limit });
    check("salesforce.com/query.search", { objects: prepared.objects, fields: prepared.fields });
    const result = sfPost("/parameterizedSearch", prepared.body).json<ApiSearchResult>();
    const records: SalesforceRecord[] = [];
    for (const raw of result.searchRecords) records.push(recordFrom(raw));
    return records;
}

/**
 * Export every record a query matches to a CSV file through the Bulk API, for
 * more records than pages can carry. Salesforce runs the job in the
 * background: while `done` is false, call again with the same query and path
 * and `{ jobId }`.
 * @param spec The query; the Bulk API does not sort, so `orderBy` is refused.
 * @param path The CSV file to write; its directory must exist.
 * @param options `jobId` of an export to continue.
 * @returns The job, its state, and once done the file and its row count.
 * @capability salesforce.com/query.export { object: string, fields: string[], path: string }
 */
export function exportQuery(spec: QuerySpec, path: string, options: ExportOptions = {}): ExportResult {
    // Copied once, field by field: the checked spec is the spec sent.
    const { object, fields, where, orderBy, limit } = spec;
    const fieldsCopy: string[] = [];
    for (const field of fields) fieldsCopy.push(field);
    const whereCopy: Condition[] = [];
    for (const condition of where ?? []) {
        const { field, op, value, values, date, dateTime, dateLiteral } = condition;
        let list: (string | number)[] | undefined = undefined;
        if (values !== undefined) {
            const items: (string | number)[] = [];
            for (const item of values) items.push(item);
            list = items;
        }
        whereCopy.push({ field: field, op: op, value: value, values: list, date: date, dateTime: dateTime, dateLiteral: dateLiteral });
    }
    const orderCopy: OrderBy[] = [];
    for (const sort of orderBy ?? []) {
        const { field, direction, nulls } = sort;
        orderCopy.push({ field: field, direction: direction, nulls: nulls });
    }
    if (orderCopy.length > 0) throw validationError("invalid_input", "exportQuery does not support orderBy");
    const prepared = buildQuery({ object: object, fields: fieldsCopy, where: whereCopy, limit: limit });
    const file = requireText(path, "path");
    const { jobId: given } = options;
    const jobId = given === undefined ? "" : normalizeId(given, "jobId");
    check("salesforce.com/query.export", { object: prepared.object, fields: prepared.fields, path: file });
    let id = jobId;
    if (id.length === 0) {
        const created = sfPost("/jobs/query", { operation: "query", query: prepared.soql, contentType: "CSV", lineEnding: "LF" });
        id = textField(created.json<Record<string, unknown>>(), "id");
    }
    let state = "";
    for (let poll = 0; poll < EXPORT_POLLS; poll += 1) {
        state = textField(sfGet("/jobs/query/" + id, 0).json<Record<string, unknown>>(), "state");
        if (state !== "UploadComplete" && state !== "InProgress") break;
    }
    if (state === "Failed" || state === "Aborted") {
        throw new SalesforceError("EXPORT_" + state.toUpperCase(), "Salesforce export job " + id + " " + state, [], 0, false);
    }
    if (state !== "JobComplete") return { jobId: id, state: state, done: false, path: file, records: 0 };
    let locator = "";
    let rows = 0;
    for (let chunk = 0; chunk < EXPORT_MAX_CHUNKS; chunk += 1) {
        let resultPath = "/jobs/query/" + id + "/results?maxRecords=" + EXPORT_CHUNK_RECORDS.toString();
        if (locator.length > 0) resultPath += "&locator=" + encodeComponent(locator);
        const response = sfGet(resultPath, 0);
        let text = response.body;
        if (chunk > 0) {
            // Every chunk repeats the header row; keep the first one only.
            const newline = text.indexOf("\n");
            text = newline < 0 ? "" : text.slice(newline + 1);
            appendText(file, text);
        } else {
            writeText(file, text);
        }
        rows += countRows(text, chunk === 0);
        locator = response.headers.get("sforce-locator") ?? "null";
        if (locator === "null" || locator.length === 0) return { jobId: id, state: state, done: true, path: file, records: rows };
    }
    throw new SalesforceError(
        "EXPORT_TOO_LARGE",
        "export job " + id + " has more than " + (EXPORT_CHUNK_RECORDS * EXPORT_MAX_CHUNKS).toString() + " records",
        [], 0, false,
    );
}

/**
 * Run a SOQL query as written. Meant for administrators: its capability has no
 * fields, so a rule cannot limit what it reads.
 * @param text The query.
 * @param page The page; pass the same query with a `cursor`.
 * @returns One page of records.
 * @capability salesforce.com/query.soql {}
 */
export function soql(text: string, page: PageOptions = {}): Page<SalesforceRecord> {
    const query = requireText(text, "text");
    const { pageSize, cursor } = page;
    const window = pageWindow({ pageSize: pageSize, cursor: cursor });
    check("salesforce.com/query.soql", {});
    const result = window.cursor === null ? queryGet(query, window.pageSize) : cursorGet(window.cursor, window.pageSize);
    return pageFrom(result, recordFrom);
}

/**
 * Run a SOSL search as written, such as `FIND {Acme} IN NAME FIELDS RETURNING Account(Id, Name)`.
 * Meant for administrators: its capability has no fields.
 * @param text The search.
 * @returns The matching records.
 * @capability salesforce.com/query.sosl {}
 */
export function sosl(text: string): SalesforceRecord[] {
    const search = requireText(text, "text");
    check("salesforce.com/query.sosl", {});
    const result = sfGet("/search/?q=" + encodeComponent(search), 0).json<ApiSearchResult>();
    const records: SalesforceRecord[] = [];
    for (const raw of result.searchRecords) records.push(recordFrom(raw));
    return records;
}

// ---------------------------------------------------------------------------
// Collaboration and activity
// ---------------------------------------------------------------------------

/**
 * Log a completed activity, such as a call or email, as a task on a record.
 * @param input The activity: its type, subject, record and person.
 * @returns The new task's id.
 * @capability salesforce.com/activities.log { type: string, object: string, accountId: string, fields: string[] }
 */
export function logActivity(input: ActivityInput): string {
    const { type, subject, whatId: what, whoId: who, description, date, custom } = input;
    const extra: FieldWrite[] = [];
    for (const write of custom ?? []) {
        const { field, value } = write;
        extra.push({ field: field, value: value });
    }
    const typed = activityFields(type);
    const whatId = optionalId(what, "whatId");
    const whoId = optionalId(who, "whoId");
    const writes = new Writes();
    for (const write of typed) writes.set(write.field, write.value);
    writes.set("Subject", requireText(subject, "subject"));
    writes.set("Status", "Completed");
    if (whatId.length > 0) writes.set("WhatId", whatId);
    if (whoId.length > 0) writes.set("WhoId", whoId);
    writes.set("Description", description);
    writes.setDate("ActivityDate", date, "date");
    const prepared = writes.prepare(extra, "input");
    const facts = activityFacts(whatId, whoId);
    check("salesforce.com/activities.log", { type: type, object: facts.key, accountId: facts.accountId, fields: prepared.keys });
    return createdId(sfPost("/sobjects/Task", prepared.body));
}

/**
 * Create a calendar event, such as a meeting, on a record.
 * @param input The event: its subject, start and end, record and person.
 * @returns The new event's id.
 * @capability salesforce.com/activities.createEvent { object: string, accountId: string, fields: string[] }
 */
export function createEvent(input: EventInput): string {
    const { subject, start, end, whatId: what, whoId: who, location, description, custom } = input;
    const extra: FieldWrite[] = [];
    for (const write of custom ?? []) {
        const { field, value } = write;
        extra.push({ field: field, value: value });
    }
    const whatId = optionalId(what, "whatId");
    const whoId = optionalId(who, "whoId");
    const writes = new Writes();
    writes.set("Subject", requireText(subject, "subject"));
    writes.set("StartDateTime", requireDateTime(start, "start"));
    writes.set("EndDateTime", requireDateTime(end, "end"));
    if (whatId.length > 0) writes.set("WhatId", whatId);
    if (whoId.length > 0) writes.set("WhoId", whoId);
    writes.set("Location", location);
    writes.set("Description", description);
    const prepared = writes.prepare(extra, "input");
    const facts = activityFacts(whatId, whoId);
    check("salesforce.com/activities.createEvent", { object: facts.key, accountId: facts.accountId, fields: prepared.keys });
    return createdId(sfPost("/sobjects/Event", prepared.body));
}

/**
 * A record's activity history (completed tasks and past events, newest first),
 * or its open activities (upcoming first). An account's include those of its
 * contacts and opportunities.
 * @param recordId The record, such as an account, contact or opportunity.
 * @param options `open` for open activities, and a `limit`.
 * @returns The activities; empty when the record does not exist.
 * @capability salesforce.com/activities.list { object: string, accountId: string, ownerId: string }
 */
export function listActivities(recordId: string, options: ActivityOptions = {}): Activity[] {
    const id = normalizeId(recordId, "recordId");
    const { open, limit } = options;
    const max = activityLimit(limit);
    const facts = recordFacts(id);
    check("salesforce.com/activities.list", { object: facts.key, accountId: facts.accountId, ownerId: facts.ownerId });
    if (!facts.exists) return [];
    const result = queryGet(activitySoql(facts.object, id, open === true, max), 200);
    return result.records.length === 0 ? [] : activitiesFrom(result.records[0], open === true);
}

/**
 * Add a note to a record.
 * @param recordId The record, such as an account or opportunity.
 * @param title The note's title.
 * @param body Its text; each line becomes a paragraph.
 * @returns The note's id (a `ContentDocument` id).
 * @capability salesforce.com/notes.create { object: string, accountId: string, ownerId: string }
 */
export function createNote(recordId: string, title: string, body: string): string {
    const id = normalizeId(recordId, "recordId");
    const noteTitle = requireText(title, "title");
    const content = noteContent(body);
    const facts = recordFacts(id);
    check("salesforce.com/notes.create", { object: facts.key, accountId: facts.accountId, ownerId: facts.ownerId });
    const created = orgPost(dataPath() + "/sobjects/ContentNote", { Title: noteTitle, Content: content }, headers(0));
    if (created.status === 404) {
        // Salesforce answers a bare NOT_FOUND when the org has Notes turned off.
        throw new SalesforceError("NOT_FOUND", "this org has Notes turned off: Setup, Notes Settings, Enable Notes", [], 404, false);
    }
    const noteId = createdId(requireOk(created));
    const linked = orgPost(dataPath() + "/sobjects/ContentDocumentLink", { ContentDocumentId: noteId, LinkedEntityId: id, ShareType: "I" }, headers(0));
    if (!linked.ok) {
        // A note on no record is not what was asked for: remove it, and report why.
        orgDelete("/sobjects/ContentNote/" + noteId);
        throw errorOf(linked);
    }
    return noteId;
}

/**
 * Upload a file from the VFS and attach it to a record.
 * @param recordId The record, such as an account, opportunity or case.
 * @param path The VFS file, at most 37 MB.
 * @param options Its `title`; default its file name.
 * @returns The file's ids.
 * @capability salesforce.com/files.upload { object: string, accountId: string, ownerId: string, path: string }
 */
export function uploadFile(recordId: string, path: string, options: UploadOptions = {}): UploadedFile {
    const id = normalizeId(recordId, "recordId");
    const file = requireText(path, "path");
    const name = fileName(file);
    if (name.length === 0) throw validationError("invalid_input", "path must name a file");
    const { title } = options;
    const fileTitle = title === undefined ? name : requireText(title, "title");
    const facts = recordFacts(id);
    check("salesforce.com/files.upload", { object: facts.key, accountId: facts.accountId, ownerId: facts.ownerId, path: file });
    const metadata = exists(file) ? stat(file) : undefined;
    if (metadata === undefined || metadata.kind !== "file") throw validationError("file_not_found", "not a VFS file: " + file);
    if (metadata.size > MAX_UPLOAD_BYTES) throw validationError("file_too_large", "files above 37 MB cannot be uploaded through this API");
    const data = read(file).toBase64();
    const versionId = createdId(sfPost("/sobjects/ContentVersion", {
        Title: fileTitle, PathOnClient: name, VersionData: data, FirstPublishLocationId: id,
    }));
    const version = lookup("SELECT Id, ContentDocumentId FROM ContentVersion", versionId);
    return { contentDocumentId: textOf(version, "ContentDocumentId"), contentVersionId: versionId };
}

/**
 * Download a file's latest version to the VFS.
 * @param contentDocumentId The file (`ContentDocument`, `069…`), as `uploadFile` or a record's `ContentDocumentLinks` give it.
 * @param path Where to write it; its directory must exist.
 * @param options `overwrite` a file already there.
 * @returns What was written.
 * @capability salesforce.com/files.download { path: string }
 */
export function downloadFile(contentDocumentId: string, path: string, options: FileDownloadOptions = {}): DownloadedFile {
    const id = normalizeId(contentDocumentId, "contentDocumentId");
    const file = requireText(path, "path");
    const { overwrite } = options;
    check("salesforce.com/files.download", { path: file });
    const document = lookup("SELECT Id, Title, FileExtension, LatestPublishedVersionId FROM ContentDocument", id);
    if (document === null) throw new SalesforceError("NOT_FOUND", "Salesforce file " + id + " was not found", [], 404, false);
    const versionId = normalizeId(textOf(document, "LatestPublishedVersionId"), "LatestPublishedVersionId");
    const result = download(ORG + dataPath() + "/sobjects/ContentVersion/" + versionId + "/VersionData", file, {
        headers: headers(0), overwrite: overwrite === true,
    });
    if (result.status < 200 || result.status >= 300) {
        // The body written is Salesforce's error, not the file.
        remove(file);
        throw new SalesforceError(statusCode(result.status), "Salesforce download failed: HTTP " + result.status.toString(), [], result.status, result.status === 429);
    }
    const extension = textOf(document, "FileExtension");
    return { path: file, bytes: result.bytesWritten, title: textOf(document, "Title"), fileExtension: extension.length > 0 ? extension : null };
}

/**
 * Post to a record's Chatter feed, optionally mentioning users.
 * @param recordId The record, such as an account, opportunity or case.
 * @param text The post.
 * @param options Users to `mention`, by id.
 * @returns The post's id.
 * @capability salesforce.com/feed.post { object: string, accountId: string, ownerId: string, mentions: string[] }
 */
export function postToFeed(recordId: string, text: string, options: PostOptions = {}): string {
    const id = normalizeId(recordId, "recordId");
    const message = requireText(text, "text");
    const { mentions: given } = options;
    const mentions: string[] = [];
    for (const user of given ?? []) {
        const userId = normalizeId(user, "mentions");
        if (!mentions.includes(userId)) mentions.push(userId);
    }
    const facts = recordFacts(id);
    check("salesforce.com/feed.post", {
        object: facts.key, accountId: facts.accountId, ownerId: facts.ownerId, mentions: sortedDistinct(mentions),
    });
    const body = { feedElementType: "FeedItem", subjectId: id, body: feedBody(message, mentions) };
    return textField(sfPost("/chatter/feed-elements", body).json<Record<string, unknown>>(), "id");
}

/**
 * Read a record's Chatter feed, newest first.
 * @param recordId The record.
 * @param options The page.
 * @returns One page of posts; empty when the record does not exist.
 * @capability salesforce.com/feed.read { object: string, accountId: string, ownerId: string }
 */
export function readFeed(recordId: string, options: FeedOptions = {}): FeedPage {
    const id = normalizeId(recordId, "recordId");
    const { pageSize, cursor } = options;
    const size = feedPageSize(pageSize);
    const token = feedCursor(cursor);
    const facts = recordFacts(id);
    check("salesforce.com/feed.read", { object: facts.key, accountId: facts.accountId, ownerId: facts.ownerId });
    if (!facts.exists) return { posts: [], nextCursor: null };
    let path = "/chatter/feeds/record/" + id + "/feed-elements?pageSize=" + size.toString();
    if (token.length > 0) path += "&page=" + encodeComponent(token);
    return feedPageFrom(sfGet(path, 0).json<Record<string, unknown>>());
}

/**
 * Send a plain-text email as the token's user, logged on a record when `relatedTo` is given.
 * The check has every recipient's address and domain, and `domain`: the one
 * domain all recipients share, or "" when they are at several.
 * @param input The recipients, subject, body and record.
 * @capability salesforce.com/email.send { to: string[], domains: string[], domain: string, object: string, accountId: string }
 */
export function sendEmail(input: EmailInput): void {
    const { to, subject, body, relatedTo } = input;
    const given: string[] = [];
    for (const address of to) given.push(address);
    const addresses = recipients(given);
    const domains: string[] = [];
    for (const address of addresses) domains.push(domainOf(address));
    const emailSubject = requireText(subject, "subject");
    const emailBody = requireText(body, "body");
    const relatedId = optionalId(relatedTo, "relatedTo");
    const facts = relatedId.length > 0 ? recordFacts(relatedId) : NO_RECORD;
    const distinctDomains = sortedDistinct(domains);
    check("salesforce.com/email.send", {
        to: sortedDistinct(addresses), domains: distinctDomains, domain: distinctDomains.length === 1 ? distinctDomains[0] : "",
        object: facts.key, accountId: facts.accountId,
    });
    const inputs: Record<string, unknown> = {
        emailAddresses: addresses.join(","), emailSubject: emailSubject, emailBody: emailBody, senderType: "CurrentUser",
    };
    if (relatedId.length > 0) {
        inputs["relatedRecordId"] = relatedId;
        inputs["logEmailOnSend"] = true;
    }
    runAction("/actions/standard/emailSimple", inputs, "sendEmail");
}

// ---------------------------------------------------------------------------
// Process and automation
// ---------------------------------------------------------------------------

/**
 * Submit a record for approval.
 * @param recordId The record, such as an opportunity or quote.
 * @param options A `comment`, the `processName`, and the first approver.
 * @returns The approval and its new work items.
 * @capability salesforce.com/approvals.submit { object: string, processName: string, accountId: string, ownerId: string }
 */
export function submitForApproval(recordId: string, options: SubmitOptions = {}): ApprovalResult {
    const id = normalizeId(recordId, "recordId");
    const { comment, processName, nextApproverIds, skipEntryCriteria } = options;
    const approvers: string[] = [];
    for (const approver of nextApproverIds ?? []) approvers.push(normalizeId(approver, "nextApproverIds"));
    const process = processName === undefined ? null : normalizeName(processName, "processName");
    if (skipEntryCriteria === true && process === null) throw validationError("invalid_input", "skipEntryCriteria needs processName");
    const facts = recordFacts(id);
    check("salesforce.com/approvals.submit", {
        object: facts.key, processName: process === null ? "" : process.key, accountId: facts.accountId, ownerId: facts.ownerId,
    });
    const request: Record<string, unknown> = { actionType: "Submit", contextId: id };
    if (comment !== undefined) request["comments"] = comment;
    if (process !== null) request["processDefinitionNameOrId"] = process.name;
    if (skipEntryCriteria === true) request["skipEntryCriteria"] = true;
    if (approvers.length > 0) request["nextApproverIds"] = approvers;
    return approvalAction(request);
}

/**
 * Approve a pending approval.
 * @param workItemId The work item, from `listPendingApprovals` or `submitForApproval`.
 * @param options A `comment`, and the next approver.
 * @returns The approval's new state.
 * @capability salesforce.com/approvals.approve { object: string, processName: string, accountId: string }
 */
export function approve(workItemId: string, options: DecisionOptions = {}): ApprovalResult {
    const id = normalizeId(workItemId, "workItemId");
    const { comment, nextApproverIds } = options;
    const approvers: string[] = [];
    for (const approver of nextApproverIds ?? []) approvers.push(normalizeId(approver, "nextApproverIds"));
    const item = workItemFacts(id);
    check("salesforce.com/approvals.approve", { object: item.object, processName: item.processName, accountId: item.accountId });
    return approvalAction(decision("Approve", id, comment, approvers));
}

/**
 * Reject a pending approval.
 * @param workItemId The work item, from `listPendingApprovals`.
 * @param options A `comment`.
 * @returns The approval's new state.
 * @capability salesforce.com/approvals.reject { object: string, processName: string, accountId: string }
 */
export function reject(workItemId: string, options: DecisionOptions = {}): ApprovalResult {
    const id = normalizeId(workItemId, "workItemId");
    const { comment } = options;
    const item = workItemFacts(id);
    check("salesforce.com/approvals.reject", { object: item.object, processName: item.processName, accountId: item.accountId });
    return approvalAction(decision("Reject", id, comment, []));
}

/**
 * List approvals waiting for a decision that the user can see, newest first.
 * @param options Only those of an `actorId`, and the page.
 * @returns One page of pending approvals.
 * @capability salesforce.com/approvals.listPending { actorId: string }
 */
export function listPendingApprovals(options: PendingOptions = {}): Page<PendingApproval> {
    const { actorId: actor, pageSize, cursor } = options;
    const actorId = optionalId(actor, "actorId");
    const window = pageWindow({ pageSize: pageSize, cursor: cursor });
    check("salesforce.com/approvals.listPending", { actorId: actorId });
    let soql = "SELECT " + PENDING_FIELDS + " FROM ProcessInstanceWorkitem";
    if (actorId.length > 0) soql += " WHERE ActorId = '" + actorId + "'";
    soql += " ORDER BY CreatedDate DESC";
    const result = window.cursor === null ? queryGet(soql, window.pageSize) : cursorGet(window.cursor, window.pageSize);
    return pageFrom(result, pendingFrom);
}

/**
 * Run an autolaunched flow by its API name, with its input variables.
 * @param name The flow's API name, such as `Escalate_Case`.
 * @param inputs Its input variables.
 * @returns Its output variables.
 * @capability salesforce.com/flows.run { flow: string, inputs: string[] }
 */
export function runFlow(name: string, inputs: FlowInput[] = []): FlowResult {
    const flow = normalizeName(name, "name");
    if (flow.name.indexOf(".") >= 0) throw validationError("invalid_name", "name must be a flow's API name, got " + JSON.stringify(name));
    const values: Record<string, unknown> = {};
    const keys: string[] = [];
    for (const input of inputs) {
        const { name: variable, value } = input;
        const key = normalizeName(variable, "inputs");
        if (key.name.indexOf(".") >= 0) throw validationError("invalid_name", "an input must be a variable name, got " + JSON.stringify(variable));
        if (keys.includes(key.key)) throw validationError("duplicate_field", "inputs repeats " + key.name);
        if (typeof value === "number" && !Number.isFinite(value)) throw validationError("invalid_input", "an input value must be finite");
        keys.push(key.key);
        values[key.name] = value;
    }
    check("salesforce.com/flows.run", { flow: flow.key, inputs: sortedDistinct(keys) });
    return flowFrom(runAction("/actions/custom/flow/" + flow.name, values, "runFlow"));
}

/**
 * Convert a lead into an account, a contact and, by default, an opportunity,
 * or merge it into an existing account and contact.
 * @param leadId The lead.
 * @param options Existing records to merge into, the opportunity, status and owner.
 * @returns The account, contact and opportunity.
 * @capability salesforce.com/leads.convert { ownerId: string, toOwnerId: string, accountId: string, contactId: string, createOpportunity: boolean }
 */
export function convertLead(leadId: string, options: ConvertOptions = {}): ConvertedLead {
    const id = normalizeId(leadId, "leadId");
    const { accountId: account, contactId: contact, createOpportunity, opportunityName, convertedStatus, ownerId: owner, sendNotificationEmail } = options;
    const accountId = optionalId(account, "accountId");
    const contactId = optionalId(contact, "contactId");
    if (contactId.length > 0 && accountId.length === 0) throw validationError("invalid_input", "contactId needs the contact's accountId");
    const toOwnerId = optionalId(owner, "ownerId");
    const opportunity = createOpportunity !== false;
    const opportunityTitle = opportunityName === undefined ? "" : requireText(opportunityName, "opportunityName");
    if (!opportunity && opportunityTitle.length > 0) throw validationError("invalid_input", "opportunityName needs createOpportunity");
    const givenStatus = convertedStatus === undefined ? "" : requireText(convertedStatus, "convertedStatus");
    const lead = lookup("SELECT Id, OwnerId FROM Lead", id);
    check("salesforce.com/leads.convert", {
        ownerId: ownerOf(lead), toOwnerId: toOwnerId, accountId: accountId, contactId: contactId, createOpportunity: opportunity,
    });
    let status = givenStatus;
    if (status.length === 0) {
        const found = queryGet("SELECT MasterLabel FROM LeadStatus WHERE IsConverted = true ORDER BY SortOrder LIMIT 1", 200);
        status = found.records.length === 0 ? "" : textField(found.records[0], "MasterLabel");
        if (status.length === 0) throw validationError("no_converted_status", "the org has no converted lead status");
    }
    const envelope = convertEnvelope(accessToken(), {
        leadId: id, accountId: accountId, contactId: contactId, convertedStatus: status, createOpportunity: opportunity,
        opportunityName: opportunityTitle, ownerId: toOwnerId, sendNotificationEmail: sendNotificationEmail === true,
    });
    const soapHeaders = new Map<string, string>();
    soapHeaders.set("Content-Type", "text/xml; charset=UTF-8");
    soapHeaders.set("SOAPAction", "\"\"");
    soapHeaders.set("User-Agent", USER_AGENT);
    const response = orgPost("services/Soap/u/" + apiVersion, envelope, soapHeaders);
    return convertedFrom(response.status, response.body);
}

/**
 * Give a record to another user or queue. The only operation that changes
 * `OwnerId`, so a rule on owners governs every reassignment.
 * @param object Object API name.
 * @param id Record id.
 * @param newOwnerId The new owner: a user (`005…`) or queue (`00G…`).
 * @capability salesforce.com/records.changeOwner { object: string, fromOwnerId: string, toOwnerId: string, accountId: string }
 */
export function changeOwner(object: string, id: string, newOwnerId: string): void {
    const name = normalizeName(object, "object");
    const recordId = normalizeId(id, "id");
    const toOwnerId = normalizeId(newOwnerId, "newOwnerId");
    const raw = fetchRecord(name.name, recordId);
    check("salesforce.com/records.changeOwner", {
        object: name.key, fromOwnerId: ownerOf(raw), toOwnerId: toOwnerId, accountId: accountOf(name.key, recordId, raw),
    });
    if (raw === null) throw new SalesforceError("NOT_FOUND", name.name + " " + recordId + " was not found", [], 404, false);
    sfPatch("/sobjects/" + name.name + "/" + recordId, { OwnerId: toOwnerId });
}

// ---------------------------------------------------------------------------
// Reports, dashboards and metadata
// ---------------------------------------------------------------------------

/**
 * List the reports the user can see, by name.
 * @param options Filter by `name` or `folder`, and the page.
 * @returns One page of reports.
 * @capability salesforce.com/reports.list {}
 */
export function listReports(options: ListOptions = {}): Page<ReportSummary> {
    const { name, folder, pageSize, cursor } = options;
    const where: string[] = [];
    if (name !== undefined) where.push("Name LIKE " + containsLiteral(requireText(name, "name")));
    if (folder !== undefined) where.push("FolderName = " + quote(requireText(folder, "folder")));
    const window = pageWindow({ pageSize: pageSize, cursor: cursor });
    check("salesforce.com/reports.list", {});
    let soql = "SELECT Id, Name, DeveloperName, FolderName, Format, Description, LastRunDate FROM Report";
    if (where.length > 0) soql += " WHERE " + where.join(" AND ");
    soql += " ORDER BY Name";
    const result = window.cursor === null ? queryGet(soql, window.pageSize) : cursorGet(window.cursor, window.pageSize);
    return pageFrom(result, (raw: Record<string, unknown>): ReportSummary => ({
        id: textField(raw, "Id"), name: textAt(raw, "Name"), developerName: textAt(raw, "DeveloperName"),
        folderName: textAt(raw, "FolderName"), format: textAt(raw, "Format"), description: textAt(raw, "Description"),
        lastRunDate: textAt(raw, "LastRunDate"),
    }));
}

/**
 * A report's columns, groupings, aggregates and saved filters, to choose `runReport` filters.
 * @param reportId The report (`00O…`).
 * @returns Its definition, or null when it does not exist.
 * @capability salesforce.com/reports.describe { reportId: string }
 */
export function describeReport(reportId: string): ReportDescription | null {
    const id = normalizeId(reportId, "reportId");
    check("salesforce.com/reports.describe", { reportId: id });
    const response = sfGetNullable("/analytics/reports/" + id + "/describe", 0);
    return response === null ? null : reportDescriptionFrom(response.json<Record<string, unknown>>());
}

/**
 * Run a report: its detail rows, flattened with their group values, and its
 * subtotals and grand total.
 * @param reportId The report (`00O…`).
 * @param options Filters to add for this run, `includeDetails`, and `raw`.
 * @returns The result; `truncated` when Salesforce cut the rows at 2000.
 * @capability salesforce.com/reports.run { reportId: string }
 */
export function runReport(reportId: string, options: RunReportOptions = {}): ReportResult {
    const id = normalizeId(reportId, "reportId");
    const { filters, includeDetails, raw } = options;
    const added: Record<string, unknown>[] = [];
    for (const filter of filters ?? []) {
        const { column, operator, value } = filter;
        added.push(reportFilter({ column: column, operator: operator, value: value }));
    }
    const details = includeDetails !== false;
    check("salesforce.com/reports.run", { reportId: id });
    const path = "/analytics/reports/" + id + "?includeDetails=" + (details ? "true" : "false");
    let response: Response;
    if (added.length === 0) {
        response = sfGet(path, 0);
    } else {
        const describe = sfGet("/analytics/reports/" + id + "/describe", 0).json<Record<string, unknown>>();
        response = sfPost(path, { reportMetadata: withFilters(objectAt(describe, "reportMetadata"), added) });
    }
    const result = reportFrom(response.json<Record<string, unknown>>(), details);
    if (raw === true) result.raw = response.body;
    return result;
}

/**
 * List the dashboards the user can see, by title.
 * @param options Filter by `name` (title) or `folder`, and the page.
 * @returns One page of dashboards.
 * @capability salesforce.com/dashboards.list {}
 */
export function listDashboards(options: ListOptions = {}): Page<DashboardSummary> {
    const { name, folder, pageSize, cursor } = options;
    const where: string[] = [];
    if (name !== undefined) where.push("Title LIKE " + containsLiteral(requireText(name, "name")));
    if (folder !== undefined) where.push("FolderName = " + quote(requireText(folder, "folder")));
    const window = pageWindow({ pageSize: pageSize, cursor: cursor });
    check("salesforce.com/dashboards.list", {});
    let soql = "SELECT Id, Title, DeveloperName, FolderName, Description, LastModifiedDate FROM Dashboard";
    if (where.length > 0) soql += " WHERE " + where.join(" AND ");
    soql += " ORDER BY Title";
    const result = window.cursor === null ? queryGet(soql, window.pageSize) : cursorGet(window.cursor, window.pageSize);
    return pageFrom(result, (raw: Record<string, unknown>): DashboardSummary => ({
        id: textField(raw, "Id"), title: textAt(raw, "Title"), developerName: textAt(raw, "DeveloperName"),
        folderName: textAt(raw, "FolderName"), description: textAt(raw, "Description"), lastModifiedDate: textAt(raw, "LastModifiedDate"),
    }));
}

/**
 * A dashboard and the totals each component shows, from its last refresh.
 * @param dashboardId The dashboard (`01Z…`).
 * @returns The dashboard, or null when it does not exist.
 * @capability salesforce.com/dashboards.get { dashboardId: string }
 */
export function getDashboard(dashboardId: string): Dashboard | null {
    const id = normalizeId(dashboardId, "dashboardId");
    check("salesforce.com/dashboards.get", { dashboardId: id });
    const response = sfGetNullable("/analytics/dashboards/" + id, 0);
    return response === null ? null : dashboardFrom(response.json<Record<string, unknown>>());
}

/**
 * List an object's list views, such as `My Opportunities`.
 * @param object Object API name.
 * @returns Its list views.
 * @capability salesforce.com/listViews.list { object: string }
 */
export function listListViews(object: string): ListView[] {
    const name = normalizeName(object, "object");
    check("salesforce.com/listViews.list", { object: name.key });
    return listViewsFrom(sfGet("/sobjects/" + name.name + "/listviews", 0).json<Record<string, unknown>>());
}

/**
 * Read a list view's records, with its columns, filters and sort.
 * @param object Object API name.
 * @param listViewId The list view (`00B…`).
 * @param page The page; pass the same arguments with a `cursor`.
 * @returns One page of records.
 * @capability salesforce.com/listViews.run { object: string, listViewId: string }
 */
export function runListView(object: string, listViewId: string, page: PageOptions = {}): Page<SalesforceRecord> {
    const name = normalizeName(object, "object");
    const id = normalizeId(listViewId, "listViewId");
    const { pageSize, cursor } = page;
    const window = pageWindow({ pageSize: pageSize, cursor: cursor });
    check("salesforce.com/listViews.run", { object: name.key, listViewId: id });
    if (window.cursor !== null) return pageFrom(cursorGet(window.cursor, window.pageSize), recordFrom);
    const describe = sfGet("/sobjects/" + name.name + "/listviews/" + id + "/describe", 0).json<Record<string, unknown>>();
    const soql = textAt(describe, "query");
    if (soql === null) throw new SalesforceError("LIST_VIEW_NOT_QUERYABLE", "list view " + id + " has no query", [], 0, false);
    return pageFrom(queryGet(soql, window.pageSize), recordFrom);
}

/**
 * List the org's objects the user can see, standard and custom.
 * @returns The objects.
 * @capability salesforce.com/objects.list {}
 */
export function listObjects(): ObjectSummary[] {
    check("salesforce.com/objects.list", {});
    return objectsFrom(sfGet("/sobjects", 0).json<Record<string, unknown>>());
}

/**
 * An object's fields (with picklist values and lookups), relationships and record types.
 * @param object Object API name.
 * @returns The description, or null when the object does not exist.
 * @capability salesforce.com/objects.describe { object: string }
 */
export function describeObject(object: string): ObjectDescription | null {
    const name = normalizeName(object, "object");
    check("salesforce.com/objects.describe", { object: name.key });
    const response = sfGetNullable("/sobjects/" + name.name + "/describe", 0);
    return response === null ? null : objectDescriptionFrom(response.json<Record<string, unknown>>());
}

/**
 * The org's API limits and what is left of them, such as `DailyApiRequests`.
 * @returns The limits, by name.
 * @capability salesforce.com/org.limits {}
 */
export function getLimits(): OrgLimit[] {
    check("salesforce.com/org.limits", {});
    return limitsFrom(sfGet("/limits", 0).json<Record<string, unknown>>());
}

/**
 * The user the token acts as.
 * @returns The user.
 * @capability salesforce.com/org.currentUser {}
 */
export function getCurrentUser(): CurrentUser {
    check("salesforce.com/org.currentUser", {});
    return currentUserFrom(sfGet("/chatter/users/me", 0).json<Record<string, unknown>>());
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

function findTyped<T>(
    object: string,
    conditions: PreparedCondition[],
    extra: ExtraFields,
    window: PageWindow,
    map: (fields: Record<string, FieldValue>, requested: ApiName[]) => T,
): Page<T> {
    const result = window.cursor === null
        ? queryGet(typedSoql(object, extra, conditions, "LastModifiedDate DESC"), window.pageSize)
        : cursorGet(window.cursor, window.pageSize);
    return pageFrom(result, (raw: Record<string, unknown>): T => map(fieldsOf(raw), extra.requested));
}

function readOne(object: string, id: string, extra: ExtraFields): Record<string, FieldValue> | null {
    const soql = typedSoql(object, extra, [{ soql: "Id = '" + id + "'", key: "id" }], "") + " LIMIT 1";
    const result = queryGet(soql, 200);
    return result.records.length === 0 ? null : fieldsOf(result.records[0]);
}

function lookup(select: string, id: string): Record<string, unknown> | null {
    const result = queryGet(select + " WHERE Id = '" + id + "' LIMIT 1", 200);
    return result.records.length === 0 ? null : result.records[0];
}

function lookupByExternalId(object: string, field: string, value: string): string {
    const result = queryGet("SELECT Id FROM " + object + " WHERE " + field + " = " + quote(value) + " LIMIT 1", 200);
    return result.records.length === 0 ? "" : textField(result.records[0], "Id");
}

function fetchRecord(object: string, id: string): Record<string, unknown> | null {
    const response = sfGetNullable("/sobjects/" + object + "/" + id, 0);
    return response === null ? null : response.json<Record<string, unknown>>();
}

// A record's owner, or "" when it has none (or does not exist).
function ownerOf(raw: Record<string, unknown> | null): string {
    return raw === null ? "" : textField(raw, "OwnerId");
}

// A record's account: its own id for an account, else its `AccountId`, or "".
function accountOf(objectKey: string, id: string, raw: Record<string, unknown> | null): string {
    if (raw === null) return "";
    return objectKey === "account" ? id : textField(raw, "AccountId");
}

function textOf(raw: Record<string, unknown> | null, field: string): string {
    return raw === null ? "" : textField(raw, field);
}

function withType(object: string, body: Record<string, unknown>): Record<string, unknown> {
    const record: Record<string, unknown> = { attributes: { type: object } };
    for (const field of Object.keys(body)) record[field] = body[field];
    return record;
}

function batchSize(count: number): void {
    if (count < 1 || count > MAX_BATCH) throw validationError("invalid_input", "a batch has 1 to 200 records, got " + count.toString());
}

function changeWindow(start: string, end: string): string {
    const from = requireDateTime(start, "start");
    const to = requireDateTime(end, "end");
    return "?start=" + encodeComponent(from) + "&end=" + encodeComponent(to);
}

function countRows(text: string, withHeader: boolean): number {
    let lines = 0;
    for (const line of text.split("\n")) {
        if (line.length > 0) lines += 1;
    }
    return withHeader && lines > 0 ? lines - 1 : lines;
}

function createdId(response: Response): string {
    return response.json<ApiSaveResult>().id ?? "";
}

function queryGet(soql: string, pageSize: number): ApiQueryResult {
    return sfGet("/query/?q=" + encodeComponent(soql), pageSize).json<ApiQueryResult>();
}

function cursorGet(cursor: string, pageSize: number): ApiQueryResult {
    return sfGet("/query/" + cursor, pageSize).json<ApiQueryResult>();
}

// The REST API's path under the org's URL, without a leading slash.
function dataPath(): string {
    return "services/data/v" + apiVersion;
}

function sfGet(path: string, pageSize: number): Response {
    return requireOk(orgGet(path, pageSize));
}

function sfGetNullable(path: string, pageSize: number): Response | null {
    const response = orgGet(path, pageSize);
    if (response.ok) return response;
    const error = errorOf(response);
    if (error.status === 404) return null;
    throw error;
}

function sfPost(path: string, body: Record<string, unknown>): Response {
    return requireOk(orgPost(dataPath() + path, body, headers(0)));
}

function sfPatch(path: string, body: Record<string, unknown>): Response {
    return requireOk(patch(ORG + dataPath() + path, body, headers(0)));
}

// One call site per verb, each under the org's URL: `ORG` and then a path
// without a leading slash, so the compiler derives the org's host into the grants.
function orgGet(path: string, pageSize: number): Response {
    return get(ORG + dataPath() + path, headers(pageSize));
}

function orgPost(path: string, body: string | Record<string, unknown>, sent: Map<string, string>): Response {
    return post(ORG + path, body, sent);
}

function orgDelete(path: string): Response {
    return httpDelete(ORG + dataPath() + path, headers(0));
}

function headers(pageSize: number): Map<string, string> {
    const map = new Map<string, string>();
    map.set("Authorization", "Bearer " + accessToken());
    map.set("Accept", "application/json");
    map.set("User-Agent", USER_AGENT);
    if (pageSize > 0) map.set("Sforce-Query-Options", "batchSize=" + pageSize.toString());
    return map;
}

function requireOk(response: Response): Response {
    if (response.ok) return response;
    throw errorOf(response);
}

function errorOf(response: Response): SalesforceError {
    const failure = failureFrom(response.status, response.statusText, response.body);
    return new SalesforceError(failure.errorCode, failure.message, failure.fields, response.status, failure.retryable);
}

// What a check needs to know about a record named only by its id.
interface RecordFacts {
    // Object API name, from the id's prefix; "" when unknown.
    object: string;
    // The same, lowercased, for the check.
    key: string;
    ownerId: string;
    accountId: string;
    exists: boolean;
}

const NO_RECORD: RecordFacts = { object: "", key: "", ownerId: "", accountId: "", exists: false };

// Id prefixes of the objects activities, notes, files and approvals are most often on.
const KNOWN_PREFIXES: string[][] = [
    ["001", "Account"], ["003", "Contact"], ["006", "Opportunity"], ["500", "Case"], ["00Q", "Lead"],
    ["800", "Contract"], ["801", "Order"], ["701", "Campaign"], ["01t", "Product2"], ["005", "User"],
    ["00T", "Task"], ["00U", "Event"], ["0Q0", "Quote"],
];

// A record's object, owner and account, read from Salesforce; a record that does
// not exist has "" for its owner and account.
function recordFacts(id: string): RecordFacts {
    const object = objectOfId(id);
    if (object.length === 0) return NO_RECORD;
    const key = object.toLowerCase();
    const raw = fetchRecord(object, id);
    return { object: object, key: key, ownerId: ownerOf(raw), accountId: accountOf(key, id, raw), exists: raw !== null };
}

// The object a record id belongs to, from its first three characters; "" when no object has that prefix.
function objectOfId(id: string): string {
    const prefix = id.slice(0, 3);
    for (const known of KNOWN_PREFIXES) {
        if (known[0] === prefix) return known[1];
    }
    for (const summary of objectsFrom(sfGet("/sobjects", 0).json<Record<string, unknown>>())) {
        if (summary.keyPrefix === prefix) return summary.name;
    }
    return "";
}

// An activity's record and account: the `whatId` record's, else the account of the `whoId` contact.
function activityFacts(whatId: string, whoId: string): RecordFacts {
    const what = whatId.length > 0 ? recordFacts(whatId) : NO_RECORD;
    if (what.accountId.length > 0 || whoId.length === 0) return what;
    const who = recordFacts(whoId);
    return { object: what.object, key: what.key, ownerId: what.ownerId, accountId: who.accountId, exists: what.exists };
}

// What an approval's check needs: the record's object and account, and the process.
interface WorkItemFacts {
    object: string;
    processName: string;
    accountId: string;
}

function workItemFacts(id: string): WorkItemFacts {
    const item = lookup(
        "SELECT Id, ProcessInstance.TargetObjectId, ProcessInstance.TargetObject.Type, " +
        "ProcessInstance.ProcessDefinition.DeveloperName FROM ProcessInstanceWorkitem",
        id,
    );
    if (item === null) return { object: "", processName: "", accountId: "" };
    const pending = pendingFrom(item);
    const object = pending.object ?? "";
    const target = pending.recordId ?? "";
    let accountId = "";
    if (object.length > 0 && target.length > 0) {
        const key = object.toLowerCase();
        accountId = accountOf(key, target, fetchRecord(object, target));
    }
    return { object: object.toLowerCase(), processName: (pending.processName ?? "").toLowerCase(), accountId: accountId };
}

function decision(action: string, workItemId: string, comment: string | undefined, approvers: string[]): Record<string, unknown> {
    const request: Record<string, unknown> = { actionType: action, contextId: workItemId };
    if (comment !== undefined) request["comments"] = comment;
    if (approvers.length > 0) request["nextApproverIds"] = approvers;
    return request;
}

function approvalAction(request: Record<string, unknown>): ApprovalResult {
    const results = sfPost("/process/approvals", approvalBody(request)).json<unknown[]>();
    const first = results.length > 0 ? results[0] : null;
    if (!flagAt(first, "success")) throw failureError(actionFailure(first, "approval"), 200);
    return approvalFrom(results);
}

// Run an invocable action with one set of inputs; a failed run throws.
function runAction(path: string, inputs: Record<string, unknown>, what: string): unknown[] {
    const results = sfPost(path, { inputs: [inputs] }).json<unknown[]>();
    const first = results.length > 0 ? results[0] : null;
    if (!flagAt(first, "isSuccess")) throw failureError(actionFailure(first, what), 200);
    return results;
}

function failureError(failure: FailureDetails, status: number): SalesforceError {
    return new SalesforceError(failure.errorCode, failure.message, failure.fields, status, failure.retryable);
}
