// A synchronous, typed client for the Attio CRM REST API (https://api.attio.com/v2).
// The API key is read from the ATTIO_API_KEY secret inside the package; it is
// never an argument and never returned. Every operation normalizes its input,
// checks the normalized values, and sends exactly those values.
//
// There are no delete operations and no schema-changing operations. Status
// attributes (such as a deal's `stage`) change only through `setStatus`, or
// through `createRecord`'s `status`, so a policy on stage changes cannot be
// bypassed with a plain update.

import { get, patch, post, put, Response } from "submilli:http";
import secrets from "submilli:secrets";
import { check } from "submilli:security";
import { AttioError, validationError } from "./errors";
import {
    buildCompanyValues,
    buildNotesQuery,
    buildPersonValues,
    buildQueryBody,
    buildValues,
    CompanyInput,
    CreateRecordInput,
    findDefinition,
    normalizeDomain,
    normalizeEmail,
    normalizeRecordId,
    normalizeSlug,
    NoteInput,
    noteWindow,
    PageOptions,
    PageWindow,
    PersonInput,
    groupValues,
    PendingValue,
    PreparedCondition,
    PreparedSort,
    prepareCondition,
    prepareSort,
    prepareValue,
    queryWindow,
    RecordQuery,
    RecordValuesInput,
    requirePlainAttributes,
    requireText,
    uniqueSlugs,
    ValueFields,
} from "./request";
import {
    ApiRecord,
    Attribute,
    attributeFrom,
    AttributeList,
    AttioObject,
    AttioRecord,
    currentStatus,
    failureFrom,
    findStatus,
    Member,
    memberFrom,
    MemberList,
    Note,
    NoteEnvelope,
    noteFrom,
    NoteList,
    ObjectList,
    objectFrom,
    Page,
    pageAfter,
    RecordEnvelope,
    recordFrom,
    RecordList,
    referencesMembers,
    Status,
    StatusChange,
    statusFrom,
    StatusList,
} from "./response";

export { AttioError } from "./errors";
export {
    AttributeValueType,
    AttributeValue,
    RecordValuesInput,
    StatusInput,
    CreateRecordInput,
    RecordReference,
    QueryOperator,
    QueryValue,
    QueryCondition,
    QuerySort,
    RecordQuery,
    PageOptions,
    PersonInput,
    CompanyInput,
    NoteInput,
} from "./request";
export { AttioObject, Attribute, Status, Member, RecordValue, AttioRecord, StatusChange, Note, Page } from "./response";

const API = "https://api.attio.com/v2";

/**
 * List the workspace's objects, such as `people`, `companies` and `deals` (when enabled), and custom objects.
 * @returns Every object, with the slug the other operations take.
 * @capability attio.com/objects.list {}
 */
export function listObjects(): AttioObject[] {
    check("attio.com/objects.list", {});
    const list = attioGet("/objects").json() as ObjectList;
    const items: AttioObject[] = [];
    for (const item of list.data) items.push(objectFrom(item));
    return items;
}

/**
 * List an object's active attributes, with their slugs and types.
 * @param object Object slug, such as `deals`.
 * @returns The non-archived attributes, in Attio's UI order.
 * @capability attio.com/attributes.list { object: string }
 */
export function listAttributes(object: string): Attribute[] {
    const slug = normalizeSlug(object, "object");
    check("attio.com/attributes.list", { object: slug });
    return fetchAttributes(slug);
}

/**
 * List the live (non-archived) statuses of a status attribute, such as the stages of `deals.stage`.
 * @param object Object slug, such as `deals`.
 * @param attribute Status attribute slug, such as `stage`.
 * @returns The statuses, in Attio's order.
 * @capability attio.com/statuses.list { object: string, attribute: string }
 */
export function listStatuses(object: string, attribute: string): Status[] {
    const slug = normalizeSlug(object, "object");
    const attributeSlug = normalizeSlug(attribute, "attribute");
    check("attio.com/statuses.list", { object: slug, attribute: attributeSlug });
    return fetchStatuses(slug, attributeSlug);
}

/**
 * List the members of the Attio workspace.
 * @returns Every workspace member, with a lowercased email.
 * @capability attio.com/members.list {}
 */
export function listMembers(): Member[] {
    check("attio.com/members.list", {});
    return fetchMembers();
}

/**
 * Get one record with its current values; null when it does not exist.
 * @param object Object slug, such as `deals`.
 * @param recordId Record id (UUID).
 * @returns The record, or null when Attio has no such record (or no such object).
 * @capability attio.com/records.get { object: string, recordId: string }
 */
export function getRecord(object: string, recordId: string): AttioRecord | null {
    const slug = normalizeSlug(object, "object");
    const id = normalizeRecordId(recordId, "recordId");
    check("attio.com/records.get", { object: slug, recordId: id });
    const response = attioGetNullable(recordPath(slug, id));
    if (response === null) return null;
    return readRecord((response.json() as RecordEnvelope).data, slug);
}

/**
 * Query an object's records with AND-combined conditions, sorts and one page of limit/offset pagination.
 * @param object Object slug, such as `deals`.
 * @param query Conditions, sorts, `limit` (1–500, default 50) and `offset` (default 0); omit it for the first 50 records.
 * @returns One page of records; pass `nextOffset` back as `offset` while `isComplete` is false.
 * @capability attio.com/records.query { object: string }
 */
export function queryRecords(object: string, query: RecordQuery = {}): Page<AttioRecord> {
    const slug = normalizeSlug(object, "object");
    const { filter, sorts, limit, offset } = query;
    const conditions: PreparedCondition[] = [];
    for (const condition of filter ?? []) conditions.push(prepareCondition(condition));
    const order: PreparedSort[] = [];
    for (const sort of sorts ?? []) order.push(prepareSort(sort));
    const window = queryWindow(limit, offset);
    check("attio.com/records.query", { object: slug });
    const list = attioPost("/objects/" + slug + "/records/query", buildQueryBody(conditions, order, window)).json() as RecordList;
    return recordPage(list.data, slug, window);
}

/**
 * Find the person with an email address; null when nobody has it.
 * @param email Email address; trimmed and lowercased before the lookup.
 * @returns The person record, or null when no person has this address.
 * @capability attio.com/people.find { email: string }
 */
export function findPersonByEmail(email: string): AttioRecord | null {
    const address = normalizeEmail(email);
    check("attio.com/people.find", { email: address });
    const body: Record<string, unknown> = { filter: { email_addresses: address }, limit: 2, offset: 0 };
    const list = attioPost("/objects/people/records/query", body).json() as RecordList;
    if (list.data.length === 0) return null;
    if (list.data.length > 1) {
        throw validationError("multiple_match_results", "more than one person in Attio has the email " + address + "; merge them in Attio");
    }
    return readRecord(list.data[0], "people");
}

/**
 * Create a person, or update the person who has this email address. Only the fields given are written.
 * @param input The email that identifies the person, and the fields to set.
 * @returns The created or updated person.
 * @capability attio.com/people.upsert { email: string }
 */
export function upsertPerson(input: PersonInput): AttioRecord {
    const { email, firstName, lastName, fullName, jobTitle, description, companyRecordId } = input;
    const address = normalizeEmail(email);
    const company = companyRecordId === undefined ? undefined : normalizeRecordId(companyRecordId, "companyRecordId");
    const values = buildPersonValues(address, firstName, lastName, fullName, jobTitle, description, company);
    check("attio.com/people.upsert", { email: address });
    const response = upsertOn("people", "email_addresses", values, "more than one person in Attio has the email " + address);
    return readRecord((response.json() as RecordEnvelope).data, "people");
}

/**
 * Create a company, or update the company that has this domain. Only the fields given are written.
 * @param input The domain that identifies the company, and the fields to set.
 * @returns The created or updated company.
 * @capability attio.com/companies.upsert { domain: string }
 */
export function upsertCompany(input: CompanyInput): AttioRecord {
    const { domain, name, description } = input;
    const bare = normalizeDomain(domain);
    const values = buildCompanyValues(bare, name, description);
    check("attio.com/companies.upsert", { domain: bare });
    const response = upsertOn("companies", "domains", values, "more than one company in Attio has the domain " + bare);
    return readRecord((response.json() as RecordEnvelope).data, "companies");
}

/**
 * Create a record, optionally in an initial status such as a deal stage.
 * @param object Object slug, such as `deals`.
 * @param input Attribute values, and at most one `status` (`{ attribute, title }`).
 * @returns The created record.
 * @capability attio.com/records.create { object: string, attributes: string[], statusAttribute: string, status: string }
 */
export function createRecord(object: string, input: CreateRecordInput): AttioRecord {
    const slug = normalizeSlug(object, "object");
    const { values, status } = input;
    // The checked slugs and the values they carry are kept apart: only the
    // slugs reach `check()`, and the request pairs each with its values.
    const written: string[] = [];
    const pending: PendingValue[] = [];
    for (const write of values ?? []) {
        const { attribute, type, text, number, amount, checked, date, option, email, domain, targetObject, targetRecordId } = write;
        const attributeSlug = normalizeSlug(attribute, "attribute");
        written.push(attributeSlug);
        const fields: ValueFields = { type, text, number, amount, checked, date, option, email, domain, targetObject, targetRecordId };
        pending.push({ slug: attributeSlug, value: prepareValue(attributeSlug, fields) });
    }
    const slugs = uniqueSlugs(written);
    const attributes = groupValues(pending);
    let statusAttribute = "";
    let requestedStatus = "";
    if (status !== undefined) {
        const { attribute, title } = status;
        statusAttribute = normalizeSlug(attribute, "status attribute");
        requestedStatus = requireText(title, "status title");
    }
    const resolved = statusAttribute.length > 0
        ? findStatus(fetchStatuses(slug, statusAttribute), requestedStatus, slug, statusAttribute)
        : null;
    const statusTitle = resolved === null ? "" : resolved.title;
    check("attio.com/records.create", { object: slug, attributes: slugs, statusAttribute: statusAttribute, status: statusTitle });
    // Decided after `check()`, so the shape of `values` never decides whether it runs.
    if (attributes.length === 0 && resolved === null) {
        throw validationError("empty_record", "createRecord needs at least one value or a status");
    }
    const definitions = fetchAttributes(slug);
    requirePlainAttributes(slug, attributes, definitions, "set it with input.status");
    const body = buildValues(attributes);
    if (resolved !== null) {
        const definition = findDefinition(definitions, statusAttribute);
        if (definition === null || definition.type !== "status") {
            throw validationError("invalid_status_attribute", statusAttribute + " is not a status attribute of " + slug);
        }
        body[statusAttribute] = [{ status: resolved.statusId }];
    }
    const response = attioPost("/objects/" + slug + "/records", { data: { values: body } });
    return readRecord((response.json() as RecordEnvelope).data, slug);
}

/**
 * Update a record's attributes. Only the given attributes change; multi-value attributes gain the given values. Status attributes are refused.
 * @param object Object slug, such as `deals`.
 * @param recordId Record id (UUID).
 * @param values Attribute values, one entry per value; change a status with `setStatus` instead.
 * @returns The updated record.
 * @capability attio.com/records.update { object: string, recordId: string, attributes: string[] }
 */
export function updateRecord(object: string, recordId: string, values: RecordValuesInput): AttioRecord {
    const slug = normalizeSlug(object, "object");
    const id = normalizeRecordId(recordId, "recordId");
    const written: string[] = [];
    const pending: PendingValue[] = [];
    for (const write of values) {
        const { attribute, type, text, number, amount, checked, date, option, email, domain, targetObject, targetRecordId } = write;
        const attributeSlug = normalizeSlug(attribute, "attribute");
        written.push(attributeSlug);
        const fields: ValueFields = { type, text, number, amount, checked, date, option, email, domain, targetObject, targetRecordId };
        pending.push({ slug: attributeSlug, value: prepareValue(attributeSlug, fields) });
    }
    const slugs = uniqueSlugs(written);
    const attributes = groupValues(pending);
    check("attio.com/records.update", { object: slug, recordId: id, attributes: slugs });
    // Decided after `check()`, so the shape of `values` never decides whether it runs.
    if (attributes.length === 0) throw validationError("empty_update", "updateRecord needs at least one attribute value");
    requirePlainAttributes(slug, attributes, fetchAttributes(slug), "change it with setStatus");
    const response = attioPatch(recordPath(slug, id), { data: { values: buildValues(attributes) } });
    return readRecord((response.json() as RecordEnvelope).data, slug);
}

/**
 * Change a record's status attribute, such as a deal's stage, to a live status given by title or id.
 * @param object Object slug, such as `deals`.
 * @param recordId Record id (UUID).
 * @param attribute Status attribute slug, such as `stage`.
 * @param status The new status: its title (case-insensitive) or status id.
 * @returns The record, the canonical new title, the previous title, and whether anything changed.
 * @capability attio.com/records.setStatus { object: string, recordId: string, attribute: string, status: string, previousStatus: string }
 */
export function setStatus(object: string, recordId: string, attribute: string, status: string): StatusChange {
    const slug = normalizeSlug(object, "object");
    const id = normalizeRecordId(recordId, "recordId");
    const attributeSlug = normalizeSlug(attribute, "attribute");
    const requested = requireText(status, "status");
    const target = findStatus(fetchStatuses(slug, attributeSlug), requested, slug, attributeSlug);
    const response = attioGetNullable(recordPath(slug, id));
    if (response === null) {
        throw new AttioError("not_found", "Attio has no " + slug + " record " + id, 404, "invalid_request_error", false, "");
    }
    const current = (response.json() as RecordEnvelope).data;
    const previous = currentStatus(current, attributeSlug);
    check("attio.com/records.setStatus", {
        object: slug,
        recordId: id,
        attribute: attributeSlug,
        status: target.title,
        previousStatus: previous.title,
    });
    if (previous.statusId === target.statusId) {
        return { record: readRecord(current, slug), status: target.title, previousStatus: previous.title, changed: false };
    }
    const values: Record<string, unknown> = {};
    values[attributeSlug] = [{ status: target.statusId }];
    const updated = attioPatch(recordPath(slug, id), { data: { values: values } });
    return {
        record: readRecord((updated.json() as RecordEnvelope).data, slug),
        status: target.title,
        previousStatus: previous.title,
        changed: true,
    };
}

/**
 * Attach a note to a record.
 * @param parentObject Object slug of the record, such as `deals`.
 * @param parentRecordId Record id (UUID).
 * @param input Title, content and format (`plaintext` by default, or `markdown`).
 * @returns The created note.
 * @capability attio.com/notes.create { parentObject: string, parentRecordId: string }
 */
export function createNote(parentObject: string, parentRecordId: string, input: NoteInput): Note {
    const parent = normalizeSlug(parentObject, "parentObject");
    const id = normalizeRecordId(parentRecordId, "parentRecordId");
    const { title, content, format } = input;
    const heading = requireText(title, "title");
    const kind = format ?? "plaintext";
    if (kind !== "plaintext" && kind !== "markdown") {
        throw validationError("invalid_input", "format must be plaintext or markdown, got " + JSON.stringify(kind));
    }
    check("attio.com/notes.create", { parentObject: parent, parentRecordId: id });
    const body: Record<string, unknown> = {
        data: { parent_object: parent, parent_record_id: id, title: heading, format: kind, content: content },
    };
    return noteFrom((attioPost("/notes", body).json() as NoteEnvelope).data);
}

/**
 * List one page of the notes attached to a record, newest first as Attio returns them.
 * @param parentObject Object slug of the record, such as `deals`.
 * @param parentRecordId Record id (UUID).
 * @param page `limit` (1–50, default 50) and `offset` (default 0); omit it for the first page.
 * @returns One page of notes; pass `nextOffset` back as `offset` while `isComplete` is false.
 * @capability attio.com/notes.list { parentObject: string, parentRecordId: string }
 */
export function listNotes(parentObject: string, parentRecordId: string, page: PageOptions = {}): Page<Note> {
    const parent = normalizeSlug(parentObject, "parentObject");
    const id = normalizeRecordId(parentRecordId, "parentRecordId");
    const { limit, offset } = page;
    const window = noteWindow(limit, offset);
    check("attio.com/notes.list", { parentObject: parent, parentRecordId: id });
    const list = attioGet("/notes" + buildNotesQuery(parent, id, window)).json() as NoteList;
    const items: Note[] = [];
    for (const note of list.data) {
        // Attio lists workspace-wide notes when the parent filter is missing, so
        // keep only this record's notes even though both filters are always sent.
        if (note.parent_record_id === id) items.push(noteFrom(note));
    }
    const end = pageAfter(window.offset, window.limit, list.data.length);
    return { items: items, nextOffset: end.nextOffset, isComplete: end.isComplete };
}

function recordPath(object: string, recordId: string): string {
    return "/objects/" + object + "/records/" + recordId;
}

function fetchAttributes(object: string): Attribute[] {
    const list = attioGet("/objects/" + object + "/attributes").json() as AttributeList;
    const items: Attribute[] = [];
    for (const item of list.data) {
        if (item.is_archived !== true) items.push(attributeFrom(item));
    }
    return items;
}

function fetchStatuses(object: string, attribute: string): Status[] {
    const list = attioGet("/objects/" + object + "/attributes/" + attribute + "/statuses").json() as StatusList;
    const items: Status[] = [];
    for (const item of list.data) {
        if (item.is_archived !== true) items.push(statusFrom(item));
    }
    return items;
}

function fetchMembers(): Member[] {
    const list = attioGet("/workspace_members").json() as MemberList;
    const items: Member[] = [];
    for (const item of list.data) items.push(memberFrom(item));
    return items;
}

function memberEmails(): Map<string, string> {
    const emails = new Map<string, string>();
    for (const member of fetchMembers()) emails.set(member.memberId, member.email);
    return emails;
}

function readRecord(data: ApiRecord, object: string): AttioRecord {
    const emails = referencesMembers(data) ? memberEmails() : new Map<string, string>();
    return recordFrom(data, object, emails);
}

function recordPage(records: ApiRecord[], object: string, window: PageWindow): Page<AttioRecord> {
    let needsMembers = false;
    for (const record of records) {
        if (referencesMembers(record)) needsMembers = true;
    }
    const emails = needsMembers ? memberEmails() : new Map<string, string>();
    const items: AttioRecord[] = [];
    for (const record of records) items.push(recordFrom(record, object, emails));
    const end = pageAfter(window.offset, window.limit, records.length);
    return { items: items, nextOffset: end.nextOffset, isComplete: end.isComplete };
}

function upsertOn(object: string, matchingAttribute: string, values: Record<string, unknown>, ambiguity: string): Response {
    const response = put(
        API + "/objects/" + object + "/records?matching_attribute=" + matchingAttribute,
        { data: { values: values } },
        authHeaders(),
    );
    if (response.ok) return response;
    const error = attioError(response);
    if (error.code === "multiple_match_results") {
        throw new AttioError(error.code, ambiguity + "; merge the duplicates in Attio, then retry (" + error.message + ")", error.status, error.type, false, "");
    }
    throw error;
}

function attioGet(path: string): Response {
    return requireOk(get(API + path, authHeaders()));
}

function attioGetNullable(path: string): Response | null {
    const response = get(API + path, authHeaders());
    if (response.ok) return response;
    const error = attioError(response);
    if (error.status === 404 && error.code === "not_found") return null;
    throw error;
}

function attioPost(path: string, body: Record<string, unknown>): Response {
    return requireOk(post(API + path, body, authHeaders()));
}

function attioPatch(path: string, body: Record<string, unknown>): Response {
    return requireOk(patch(API + path, body, authHeaders()));
}

function authHeaders(): Map<string, string> {
    const token = secrets.get("ATTIO_API_KEY");
    if (token === undefined) throw validationError("missing_token", "ATTIO_API_KEY is not bound");
    const headers = new Map<string, string>();
    headers.set("Authorization", "Bearer " + token);
    headers.set("Accept", "application/json");
    headers.set("User-Agent", "submilli-attio/0.1.0");
    return headers;
}

function requireOk(response: Response): Response {
    if (response.ok) return response;
    throw attioError(response);
}

function attioError(response: Response): AttioError {
    const failure = failureFrom(response.status, response.statusText, response.body);
    return new AttioError(
        failure.code,
        failure.message,
        response.status,
        failure.type,
        failure.retryable,
        response.headers.get("retry-after") ?? "",
    );
}
