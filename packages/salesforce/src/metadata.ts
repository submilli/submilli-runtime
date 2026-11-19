// Metadata: objects and their fields, list views, the org's limits and the
// current user. The result types and their mappers. Internal: `src/lib.ts`
// re-exports the types.

import { arrayAt, flagAt, numberAt, objectAt, textAt } from "./response";

/** An object of the org, as listed. */
export interface ObjectSummary {
    /** API name, such as `Account` or `Invoice__c`. */
    name: string;
    /** Label. */
    label: string;
    /** Plural label. */
    labelPlural: string;
    /** The first three characters of its records' ids, such as `001`; null when it has none. */
    keyPrefix: string | null;
    /** True for a custom object. */
    custom: boolean;
    /** True when the user may query it. */
    queryable: boolean;
    /** True when the user may create its records. */
    createable: boolean;
    /** True when the user may update its records. */
    updateable: boolean;
}

/** An object's fields, relationships and record types. */
export interface ObjectDescription {
    /** API name. */
    name: string;
    /** Label. */
    label: string;
    /** Plural label. */
    labelPlural: string;
    /** Id prefix. */
    keyPrefix: string | null;
    /** True for a custom object. */
    custom: boolean;
    /** The fields the user may see. */
    fields: FieldDescription[];
    /** Relationships from other objects to this one, such as an account's `Contacts`. */
    childRelationships: ChildRelationship[];
    /** Its record types. */
    recordTypes: RecordType[];
}

/** A field of an object. */
export interface FieldDescription {
    /** API name, such as `StageName` or `Region__c`. */
    name: string;
    /** Label. */
    label: string;
    /** Such as `string`, `picklist`, `reference`, `currency`, `date` or `boolean`. */
    type: string;
    /** True for a custom field. */
    custom: boolean;
    /** True when it may be empty. */
    nillable: boolean;
    /** True when the user may set it on create. */
    createable: boolean;
    /** True when the user may change it. */
    updateable: boolean;
    /** True when it can be used in `where`. */
    filterable: boolean;
    /** Maximum length of a text field; 0 otherwise. */
    length: number;
    /** The objects a lookup may point to. */
    referenceTo: string[];
    /** A lookup's relationship name, for paths such as `Account.Name`; null otherwise. */
    relationshipName: string | null;
    /** A picklist's active values. */
    picklistValues: string[];
    /** True for an external id field, usable with `upsertRecord`. */
    externalId: boolean;
    /** True when values must be unique. */
    unique: boolean;
}

/** A relationship from another object. */
export interface ChildRelationship {
    /** Its name, for `getRelated`, such as `Contacts`; null when it cannot be queried by name. */
    relationshipName: string | null;
    /** The other object, such as `Contact`. */
    childObject: string;
    /** The other object's lookup field, such as `AccountId`. */
    field: string;
}

/** A record type. */
export interface RecordType {
    /** Record type id, for `RecordTypeId`. */
    id: string;
    /** Name. */
    name: string;
    /** API name. */
    developerName: string;
    /** True when it is active. */
    active: boolean;
}

/** A list view of an object. */
export interface ListView {
    /** List view id, for `runListView`. */
    id: string;
    /** Label, such as `My Opportunities`. */
    label: string;
    /** API name. */
    developerName: string;
}

/** One of the org's API limits. */
export interface OrgLimit {
    /** Name, such as `DailyApiRequests`. */
    name: string;
    /** The limit. */
    max: number;
    /** What is left. */
    remaining: number;
}

/** The user the token acts as. */
export interface CurrentUser {
    /** User id. */
    id: string;
    /** Full name. */
    name: string;
    /** Username. */
    username: string | null;
    /** Email address. */
    email: string | null;
    /** Title. */
    title: string | null;
    /** Company name. */
    companyName: string | null;
}

/**
 * The objects in `GET sobjects`.
 * @param body Salesforce's answer.
 * @returns The objects.
 */
export function objectsFrom(body: Record<string, unknown>): ObjectSummary[] {
    const out: ObjectSummary[] = [];
    for (const item of arrayAt(body, "sobjects")) {
        out.push({
            name: textAt(item, "name") ?? "",
            label: textAt(item, "label") ?? "",
            labelPlural: textAt(item, "labelPlural") ?? "",
            keyPrefix: textAt(item, "keyPrefix"),
            custom: flagAt(item, "custom"),
            queryable: flagAt(item, "queryable"),
            createable: flagAt(item, "createable"),
            updateable: flagAt(item, "updateable"),
        });
    }
    return out;
}

/**
 * An object's describe.
 * @param body Salesforce's answer.
 * @returns The description.
 */
export function objectDescriptionFrom(body: Record<string, unknown>): ObjectDescription {
    const fields: FieldDescription[] = [];
    for (const field of arrayAt(body, "fields")) {
        const referenceTo: string[] = [];
        for (const target of arrayAt(field, "referenceTo")) {
            if (typeof target === "string") referenceTo.push(target);
        }
        const picklist: string[] = [];
        for (const entry of arrayAt(field, "picklistValues")) {
            const value = textAt(entry, "value");
            if (value !== null && flagAt(entry, "active")) picklist.push(value);
        }
        fields.push({
            name: textAt(field, "name") ?? "",
            label: textAt(field, "label") ?? "",
            type: textAt(field, "type") ?? "",
            custom: flagAt(field, "custom"),
            nillable: flagAt(field, "nillable"),
            createable: flagAt(field, "createable"),
            updateable: flagAt(field, "updateable"),
            filterable: flagAt(field, "filterable"),
            length: numberAt(field, "length") ?? 0,
            referenceTo: referenceTo,
            relationshipName: textAt(field, "relationshipName"),
            picklistValues: picklist,
            externalId: flagAt(field, "externalId"),
            unique: flagAt(field, "unique"),
        });
    }
    const children: ChildRelationship[] = [];
    for (const child of arrayAt(body, "childRelationships")) {
        children.push({
            relationshipName: textAt(child, "relationshipName"),
            childObject: textAt(child, "childSObject") ?? "",
            field: textAt(child, "field") ?? "",
        });
    }
    const recordTypes: RecordType[] = [];
    for (const type of arrayAt(body, "recordTypeInfos")) {
        recordTypes.push({
            id: textAt(type, "recordTypeId") ?? "",
            name: textAt(type, "name") ?? "",
            developerName: textAt(type, "developerName") ?? "",
            active: flagAt(type, "active"),
        });
    }
    return {
        name: textAt(body, "name") ?? "",
        label: textAt(body, "label") ?? "",
        labelPlural: textAt(body, "labelPlural") ?? "",
        keyPrefix: textAt(body, "keyPrefix"),
        custom: flagAt(body, "custom"),
        fields: fields,
        childRelationships: children,
        recordTypes: recordTypes,
    };
}

/**
 * The list views in `GET sobjects/{object}/listviews`.
 * @param body Salesforce's answer.
 * @returns The list views.
 */
export function listViewsFrom(body: Record<string, unknown>): ListView[] {
    const out: ListView[] = [];
    for (const view of arrayAt(body, "listviews")) {
        out.push({ id: textAt(view, "id") ?? "", label: textAt(view, "label") ?? "", developerName: textAt(view, "developerName") ?? "" });
    }
    return out;
}

/**
 * The limits in `GET limits`.
 * @param body Salesforce's answer, by limit name.
 * @returns The limits, by name.
 */
export function limitsFrom(body: Record<string, unknown>): OrgLimit[] {
    const out: OrgLimit[] = [];
    const names = Object.keys(body);
    names.sort();
    for (const name of names) {
        const limit = objectAt(body, name);
        out.push({ name: name, max: numberAt(limit, "Max") ?? 0, remaining: numberAt(limit, "Remaining") ?? 0 });
    }
    return out;
}

/**
 * The user in `GET chatter/users/me`.
 * @param body Salesforce's answer.
 * @returns The user.
 */
export function currentUserFrom(body: Record<string, unknown>): CurrentUser {
    return {
        id: textAt(body, "id") ?? "",
        name: textAt(body, "name") ?? textAt(body, "displayName") ?? "",
        username: textAt(body, "username"),
        email: textAt(body, "email"),
        title: textAt(body, "title"),
        companyName: textAt(body, "companyName"),
    };
}
