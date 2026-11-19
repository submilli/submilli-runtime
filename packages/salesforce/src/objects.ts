// Typed standard objects: their result types after Salesforce's documented
// fields, the filters of their finds, the inputs of their writes, and the
// mapping between them and the API's field names. Internal: `src/lib.ts`
// re-exports the types and runs the operations.
//
// Typed reads select `FIELDS(STANDARD)`, which returns the standard fields the
// user may see, so a field hidden by field-level security reads as null rather
// than failing the query.

import { validationError } from "./errors";
import { containsLiteral, PreparedCondition, quote, whereClause } from "./soql";
import {
    ApiName,
    FieldValue,
    FieldWrite,
    normalizeId,
    normalizePath,
    PreparedValues,
    prepareValues,
    requireDate,
    requireDateTime,
    requireText,
    sortedDistinct,
} from "./request";

/** Fields every typed record has. */
export interface StandardRecord {
    /** 18-character record id. */
    id: string;
    /** ISO date-time the record was created. */
    createdDate: string | null;
    /** ISO date-time of its last change. */
    lastModifiedDate: string | null;
    /** The extra fields asked for with `fields`, keyed as given, such as `{ Region__c: "EMEA" }`; null where the record has no value. */
    custom: Record<string, FieldValue>;
}

/** An account: a company or organization. */
export interface Account extends StandardRecord {
    /** `Name`. */
    name: string | null;
    /** `Type`: picklist, such as `Customer - Direct`. */
    type: string | null;
    /** `Industry`. */
    industry: string | null;
    /** `OwnerId`. */
    ownerId: string | null;
    /** `Owner.Name`. */
    ownerName: string | null;
    /** `ParentId`: the parent account's id. */
    parentId: string | null;
    /** `Parent.Name`. */
    parentName: string | null;
    /** `Website`. */
    website: string | null;
    /** `Phone`. */
    phone: string | null;
    /** `BillingCity`. */
    billingCity: string | null;
    /** `BillingState`. */
    billingState: string | null;
    /** `BillingCountry`. */
    billingCountry: string | null;
    /** `AnnualRevenue`. */
    annualRevenue: number | null;
    /** `NumberOfEmployees`. */
    numberOfEmployees: number | null;
    /** `AccountNumber`. */
    accountNumber: string | null;
    /** `Description`. */
    description: string | null;
    /** `LastActivityDate`: date (`YYYY-MM-DD`) of the latest activity. */
    lastActivityDate: string | null;
}

/** A contact: a person at an account. */
export interface Contact extends StandardRecord {
    /** `AccountId`. */
    accountId: string | null;
    /** `Account.Name`. */
    accountName: string | null;
    /** `FirstName`. */
    firstName: string | null;
    /** `LastName`. */
    lastName: string | null;
    /** `Name`. */
    name: string | null;
    /** `Email`. */
    email: string | null;
    /** `Phone`. */
    phone: string | null;
    /** `MobilePhone`. */
    mobilePhone: string | null;
    /** `Title`. */
    title: string | null;
    /** `Department`. */
    department: string | null;
    /** `OwnerId`. */
    ownerId: string | null;
    /** `Owner.Name`. */
    ownerName: string | null;
    /** `MailingCity`. */
    mailingCity: string | null;
    /** `MailingCountry`. */
    mailingCountry: string | null;
    /** `LeadSource`. */
    leadSource: string | null;
    /** `Description`. */
    description: string | null;
}

/** A lead: a prospect not yet converted. */
export interface Lead extends StandardRecord {
    /** `FirstName`. */
    firstName: string | null;
    /** `LastName`. */
    lastName: string | null;
    /** `Name`. */
    name: string | null;
    /** `Company`. */
    company: string | null;
    /** `Email`. */
    email: string | null;
    /** `Phone`. */
    phone: string | null;
    /** `Title`. */
    title: string | null;
    /** `Status`: picklist, such as `Open - Not Contacted`. */
    status: string | null;
    /** `LeadSource`. */
    leadSource: string | null;
    /** `Industry`. */
    industry: string | null;
    /** `Hot`, `Warm` or `Cold`. */
    rating: string | null;
    /** `OwnerId`: the owning user or queue. */
    ownerId: string | null;
    /** `Owner.Name`. */
    ownerName: string | null;
    /** `IsConverted`. */
    isConverted: boolean;
    /** `ConvertedAccountId`. */
    convertedAccountId: string | null;
    /** `ConvertedContactId`. */
    convertedContactId: string | null;
    /** `ConvertedOpportunityId`. */
    convertedOpportunityId: string | null;
    /** `ConvertedDate`. */
    convertedDate: string | null;
    /** `Description`. */
    description: string | null;
}

/** An opportunity: a deal in the pipeline. */
export interface Opportunity extends StandardRecord {
    /** `Name`. */
    name: string | null;
    /** `AccountId`. */
    accountId: string | null;
    /** `Account.Name`. */
    accountName: string | null;
    /** `OwnerId`. */
    ownerId: string | null;
    /** `Owner.Name`. */
    ownerName: string | null;
    /** `StageName`: picklist, such as `Prospecting` or `Closed Won`. */
    stage: string | null;
    /** `Amount`. */
    amount: number | null;
    /** `CloseDate`: date (`YYYY-MM-DD`). */
    closeDate: string | null;
    /** `Probability`: percent, 0–100. */
    probability: number | null;
    /** `Type`. */
    type: string | null;
    /** `LeadSource`. */
    leadSource: string | null;
    /** `NextStep`. */
    nextStep: string | null;
    /** `Pipeline`, `Best Case`, `Commit`, `Omitted` or `Closed`. */
    forecastCategory: string | null;
    /** `IsClosed`. */
    isClosed: boolean;
    /** `IsWon`. */
    isWon: boolean;
    /** `Description`. */
    description: string | null;
    /** `LastActivityDate`. */
    lastActivityDate: string | null;
}

/** A product on an opportunity. */
export interface OpportunityLineItem extends StandardRecord {
    /** `OpportunityId`. */
    opportunityId: string | null;
    /** `Product2Id`. */
    product2Id: string | null;
    /** `Product2.Name`. */
    productName: string | null;
    /** `PricebookEntryId`. */
    pricebookEntryId: string | null;
    /** `Quantity`. */
    quantity: number | null;
    /** `UnitPrice`. */
    unitPrice: number | null;
    /** `ListPrice`. */
    listPrice: number | null;
    /** `TotalPrice`. */
    totalPrice: number | null;
    /** `Discount`: percent. */
    discount: number | null;
    /** `ServiceDate`. */
    serviceDate: string | null;
    /** `Description`. */
    description: string | null;
}

/** A product. */
export interface Product extends StandardRecord {
    /** `Name`. */
    name: string | null;
    /** `ProductCode`. */
    productCode: string | null;
    /** `Family`. */
    family: string | null;
    /** `IsActive`. */
    isActive: boolean;
    /** `Description`. */
    description: string | null;
}

/** A price book. */
export interface Pricebook extends StandardRecord {
    /** `Name`. */
    name: string | null;
    /** `IsActive`. */
    isActive: boolean;
    /** `IsStandard`. */
    isStandard: boolean;
    /** `Description`. */
    description: string | null;
}

/** A support case. */
export interface Case extends StandardRecord {
    /** `CaseNumber`. */
    caseNumber: string | null;
    /** `Subject`. */
    subject: string | null;
    /** `Status`: picklist, such as `New`, `Working` or `Closed`. */
    status: string | null;
    /** `Priority`. */
    priority: string | null;
    /** `Origin`: picklist, such as `Email`, `Phone` or `Web`. */
    origin: string | null;
    /** `Type`. */
    type: string | null;
    /** `Reason`. */
    reason: string | null;
    /** `AccountId`. */
    accountId: string | null;
    /** `Account.Name`. */
    accountName: string | null;
    /** `ContactId`. */
    contactId: string | null;
    /** `Contact.Name`. */
    contactName: string | null;
    /** `OwnerId`: the owning user or queue. */
    ownerId: string | null;
    /** `Owner.Name`. */
    ownerName: string | null;
    /** `IsClosed`. */
    isClosed: boolean;
    /** `ClosedDate`. */
    closedDate: string | null;
    /** `Description`. */
    description: string | null;
}

/** A task: a to-do or a logged call or email. */
export interface Task extends StandardRecord {
    /** `Subject`. */
    subject: string | null;
    /** `Status`. */
    status: string | null;
    /** `Priority`. */
    priority: string | null;
    /** `ActivityDate`: due date (`YYYY-MM-DD`). */
    activityDate: string | null;
    /** `WhoId`: the person (contact or lead). */
    whoId: string | null;
    /** `Who.Name`. */
    whoName: string | null;
    /** `WhatId`: the record it relates to, such as an account or opportunity. */
    whatId: string | null;
    /** `What.Name`. */
    whatName: string | null;
    /** `AccountId`. */
    accountId: string | null;
    /** `OwnerId`. */
    ownerId: string | null;
    /** `Owner.Name`. */
    ownerName: string | null;
    /** `IsClosed`. */
    isClosed: boolean;
    /** `Task`, `Email`, `Call` or `LinkedIn`. */
    taskSubtype: string | null;
    /** `Type`; null where the org hides the field, as it does by default. */
    type: string | null;
    /** `Description`. */
    description: string | null;
}

/** A calendar event. */
export interface Event extends StandardRecord {
    /** `Subject`. */
    subject: string | null;
    /** `StartDateTime`. */
    startDateTime: string | null;
    /** `EndDateTime`. */
    endDateTime: string | null;
    /** `IsAllDayEvent`. */
    isAllDayEvent: boolean;
    /** `Location`. */
    location: string | null;
    /** `WhoId`. */
    whoId: string | null;
    /** `Who.Name`. */
    whoName: string | null;
    /** `WhatId`. */
    whatId: string | null;
    /** `What.Name`. */
    whatName: string | null;
    /** `AccountId`. */
    accountId: string | null;
    /** `OwnerId`. */
    ownerId: string | null;
    /** `Owner.Name`. */
    ownerName: string | null;
    /** `Type`; null where the org hides the field, as it does by default. */
    type: string | null;
    /** `Description`. */
    description: string | null;
}

/** A user of the org. */
export interface User extends StandardRecord {
    /** `Name`. */
    name: string | null;
    /** `Username`. */
    username: string | null;
    /** `Email`. */
    email: string | null;
    /** `Title`. */
    title: string | null;
    /** `Department`. */
    department: string | null;
    /** `IsActive`. */
    isActive: boolean;
    /** `Profile.Name`. */
    profileName: string | null;
    /** `UserRole.Name`. */
    roleName: string | null;
    /** `ManagerId`. */
    managerId: string | null;
}

/** A marketing campaign. */
export interface Campaign extends StandardRecord {
    /** `Name`. */
    name: string | null;
    /** `Type`. */
    type: string | null;
    /** `Status`. */
    status: string | null;
    /** `StartDate`. */
    startDate: string | null;
    /** `EndDate`. */
    endDate: string | null;
    /** `IsActive`. */
    isActive: boolean;
    /** `BudgetedCost`. */
    budgetedCost: number | null;
    /** `ActualCost`. */
    actualCost: number | null;
    /** `ExpectedRevenue`. */
    expectedRevenue: number | null;
    /** `NumberOfLeads`. */
    numberOfLeads: number | null;
    /** `NumberOfContacts`. */
    numberOfContacts: number | null;
    /** `NumberOfOpportunities`. */
    numberOfOpportunities: number | null;
    /** `OwnerId`. */
    ownerId: string | null;
    /** `Owner.Name`. */
    ownerName: string | null;
    /** `ParentId`. */
    parentId: string | null;
}

/** A lead or contact in a campaign. */
export interface CampaignMember extends StandardRecord {
    /** `CampaignId`. */
    campaignId: string | null;
    /** `Campaign.Name`. */
    campaignName: string | null;
    /** `LeadId`. */
    leadId: string | null;
    /** `ContactId`. */
    contactId: string | null;
    /** `Status`. */
    status: string | null;
    /** `HasResponded`. */
    hasResponded: boolean;
    /** `FirstRespondedDate`. */
    firstRespondedDate: string | null;
}

/** A contract with an account. */
export interface Contract extends StandardRecord {
    /** `AccountId`. */
    accountId: string | null;
    /** `Account.Name`. */
    accountName: string | null;
    /** `ContractNumber`. */
    contractNumber: string | null;
    /** `Status`. */
    status: string | null;
    /** `StartDate`. */
    startDate: string | null;
    /** `EndDate`. */
    endDate: string | null;
    /** `ContractTerm`: months. */
    contractTerm: number | null;
    /** `OwnerId`. */
    ownerId: string | null;
    /** `Owner.Name`. */
    ownerName: string | null;
    /** `Description`. */
    description: string | null;
}

/** An order. */
export interface Order extends StandardRecord {
    /** `AccountId`. */
    accountId: string | null;
    /** `Account.Name`. */
    accountName: string | null;
    /** `OrderNumber`. */
    orderNumber: string | null;
    /** `Status`. */
    status: string | null;
    /** `EffectiveDate`. */
    effectiveDate: string | null;
    /** `EndDate`. */
    endDate: string | null;
    /** `TotalAmount`. */
    totalAmount: number | null;
    /** `ContractId`. */
    contractId: string | null;
    /** `OwnerId`. */
    ownerId: string | null;
    /** `Owner.Name`. */
    ownerName: string | null;
    /** `Type`. */
    type: string | null;
    /** `Description`. */
    description: string | null;
}

/** Filters of `findAccounts`. Every filter given must match. */
export interface AccountFilters {
    /** Text the name contains. */
    name?: string;
    /** Only records whose `OwnerId` is this id. */
    ownerId?: string;
    /** Only records whose `Industry` is this. */
    industry?: string;
    /** Only records whose `Type` is this. */
    type?: string;
    /** Changed after this date-time. */
    modifiedAfter?: string;
}

/** Filters of `findContacts`. */
export interface ContactFilters {
    /** Only records whose `AccountId` is this id. */
    accountId?: string;
    /** Exact email address (Salesforce compares without regard to case). */
    email?: string;
    /** Text the full name contains. */
    name?: string;
}

/** Filters of `findLeads`. */
export interface LeadFilters {
    /** Only records whose `OwnerId` is this id. */
    ownerId?: string;
    /** Only records whose `Status` is this. */
    status?: string;
    /** Only records whose `Email` is this. */
    email?: string;
    /** Text the company contains. */
    company?: string;
    /** Only records whose `IsConverted` is this. */
    isConverted?: boolean;
}

/** Filters of `findOpportunities`. */
export interface OpportunityFilters {
    /** Only records whose `AccountId` is this id. */
    accountId?: string;
    /** Only records whose `OwnerId` is this id. */
    ownerId?: string;
    /** Exact stage name, such as `Negotiation/Review`. */
    stage?: string;
    /** Only records whose `IsClosed` is this. */
    isClosed?: boolean;
    /** Only records whose `IsWon` is this. */
    isWon?: boolean;
    /** Close date on or after (`YYYY-MM-DD`). */
    closeAfter?: string;
    /** Close date on or before (`YYYY-MM-DD`). */
    closeBefore?: string;
    /** Only records whose `Amount` is this. */
    minAmount?: number;
}

/** Filters of `findCases`. */
export interface CaseFilters {
    /** Only records whose `AccountId` is this id. */
    accountId?: string;
    /** Only records whose `OwnerId` is this id. */
    ownerId?: string;
    /** Only records whose `ContactId` is this id. */
    contactId?: string;
    /** Only records whose `Status` is this. */
    status?: string;
    /** Only records whose `Priority` is this. */
    priority?: string;
    /** Only records whose `IsClosed` is this. */
    isClosed?: boolean;
    /** Created after this date-time. */
    createdAfter?: string;
}

/** Filters of `findOpportunityLineItems`. */
export interface OpportunityLineItemFilters {
    /** Only records whose `OpportunityId` is this id. */
    opportunityId: string;
}

/** Filters of `findProducts`. */
export interface ProductFilters {
    /** Text the name contains. */
    name?: string;
    /** Only records whose `Family` is this. */
    family?: string;
    /** Only records whose `IsActive` is this. */
    isActive?: boolean;
}

/** Filters of `findPricebooks`. */
export interface PricebookFilters {
    /** Only records whose `IsActive` is this. */
    isActive?: boolean;
    /** Only records whose `IsStandard` is this. */
    isStandard?: boolean;
}

/** Filters of `findTasks`. */
export interface TaskFilters {
    /** Only records whose `WhatId` is this id. */
    whatId?: string;
    /** Only records whose `WhoId` is this id. */
    whoId?: string;
    /** Only records whose `OwnerId` is this id. */
    ownerId?: string;
    /** Only records whose `Status` is this. */
    status?: string;
    /** Only records whose `IsClosed` is this. */
    isClosed?: boolean;
    /** Due on or after (`YYYY-MM-DD`). */
    dueAfter?: string;
    /** Due on or before (`YYYY-MM-DD`). */
    dueBefore?: string;
}

/** Filters of `findEvents`. */
export interface EventFilters {
    /** Only records whose `WhatId` is this id. */
    whatId?: string;
    /** Only records whose `WhoId` is this id. */
    whoId?: string;
    /** Only records whose `OwnerId` is this id. */
    ownerId?: string;
    /** Starting at or after this date-time. */
    startAfter?: string;
    /** Starting at or before this date-time. */
    startBefore?: string;
}

/** Filters of `findUsers`. */
export interface UserFilters {
    /** Text the full name contains. */
    name?: string;
    /** Only records whose `Email` is this. */
    email?: string;
    /** Only records whose `IsActive` is this. */
    isActive?: boolean;
}

/** Filters of `findCampaigns`. */
export interface CampaignFilters {
    /** Text the name contains. */
    name?: string;
    /** Only records whose `OwnerId` is this id. */
    ownerId?: string;
    /** Only records whose `Status` is this. */
    status?: string;
    /** Only records whose `Type` is this. */
    type?: string;
    /** Only records whose `IsActive` is this. */
    isActive?: boolean;
}

/** Filters of `findCampaignMembers`. */
export interface CampaignMemberFilters {
    /** Only records whose `CampaignId` is this id. */
    campaignId?: string;
    /** Only records whose `LeadId` is this id. */
    leadId?: string;
    /** Only records whose `ContactId` is this id. */
    contactId?: string;
}

/** Filters of `findContracts` and `findOrders`. */
export interface AccountRecordFilters {
    /** Only records whose `AccountId` is this id. */
    accountId?: string;
    /** Only records whose `OwnerId` is this id. */
    ownerId?: string;
    /** Only records whose `Status` is this. */
    status?: string;
}

/** A new contact. `custom` sets other fields by API name, such as `[{ field: "Region__c", value: "EMEA" }]`. */
export interface ContactInput {
    /** `LastName`. */
    lastName: string;
    /** `FirstName`. */
    firstName?: string;
    /** `AccountId`. */
    accountId?: string;
    /** `Email`. */
    email?: string;
    /** `Phone`. */
    phone?: string;
    /** `MobilePhone`. */
    mobilePhone?: string;
    /** `Title`. */
    title?: string;
    /** `Department`. */
    department?: string;
    /** `LeadSource`. */
    leadSource?: string;
    /** `Description`. */
    description?: string;
    /** The owner; default the token's user. */
    ownerId?: string;
    /** Other fields to write, by API name, such as custom fields. */
    custom?: FieldWrite[];
}

/** A new lead. */
export interface LeadInput {
    /** `LastName`. */
    lastName: string;
    /** `Company`. */
    company: string;
    /** `FirstName`. */
    firstName?: string;
    /** `Email`. */
    email?: string;
    /** `Phone`. */
    phone?: string;
    /** `Title`. */
    title?: string;
    /** Default the org's default lead status. */
    status?: string;
    /** `LeadSource`. */
    leadSource?: string;
    /** `Industry`. */
    industry?: string;
    /** `Rating`. */
    rating?: string;
    /** `Description`. */
    description?: string;
    /** The owner; default the token's user, or the org's assignment rules. */
    ownerId?: string;
    /** Other fields to write, by API name, such as custom fields. */
    custom?: FieldWrite[];
}

/** Changes to a lead. A field left out keeps its value; `null` clears it. The owner changes only through `changeOwner`. */
export interface LeadUpdate {
    /** `FirstName`. */
    firstName?: string | null;
    /** `LastName`. */
    lastName?: string;
    /** `Company`. */
    company?: string;
    /** `Email`. */
    email?: string | null;
    /** `Phone`. */
    phone?: string | null;
    /** `Title`. */
    title?: string | null;
    /** `Status`. */
    status?: string;
    /** `LeadSource`. */
    leadSource?: string | null;
    /** `Industry`. */
    industry?: string | null;
    /** `Rating`. */
    rating?: string | null;
    /** `Description`. */
    description?: string | null;
    /** Other fields to write, by API name, such as custom fields. */
    custom?: FieldWrite[];
}

/** Changes to an opportunity. A field left out keeps its value; `null` clears it. */
export interface OpportunityUpdate {
    /** `Name`. */
    name?: string;
    /** `StageName`. */
    stage?: string;
    /** `YYYY-MM-DD`. */
    closeDate?: string;
    /** `Amount`. */
    amount?: number | null;
    /** `Probability`. */
    probability?: number | null;
    /** `NextStep`. */
    nextStep?: string | null;
    /** `Type`. */
    type?: string | null;
    /** `LeadSource`. */
    leadSource?: string | null;
    /** `Description`. */
    description?: string | null;
    /** Other fields to write, by API name, such as custom fields. */
    custom?: FieldWrite[];
}

/** A new case. */
export interface CaseInput {
    /** `Subject`. */
    subject: string;
    /** `Description`. */
    description?: string;
    /** Default the org's default case status. */
    status?: string;
    /** `Priority`. */
    priority?: string;
    /** `Origin`. */
    origin?: string;
    /** `Type`. */
    type?: string;
    /** `Reason`. */
    reason?: string;
    /** `AccountId`. */
    accountId?: string;
    /** `ContactId`. */
    contactId?: string;
    /** The owner; default the token's user, or the org's assignment rules. */
    ownerId?: string;
    /** Other fields to write, by API name, such as custom fields. */
    custom?: FieldWrite[];
}

/** Changes to a case. A field left out keeps its value; `null` clears it. */
export interface CaseUpdate {
    /** `Subject`. */
    subject?: string | null;
    /** `Description`. */
    description?: string | null;
    /** `Status`. */
    status?: string;
    /** `Priority`. */
    priority?: string | null;
    /** `Origin`. */
    origin?: string | null;
    /** `Type`. */
    type?: string | null;
    /** `Reason`. */
    reason?: string | null;
    /** `ContactId`. */
    contactId?: string | null;
    /** Other fields to write, by API name, such as custom fields. */
    custom?: FieldWrite[];
}

/** Relationship fields each typed read selects beside `FIELDS(STANDARD)`. */
export const RELATIONS: Record<string, string[]> = {
    Account: ["Owner.Name", "Parent.Name"],
    Contact: ["Account.Name", "Owner.Name"],
    Lead: ["Owner.Name"],
    Opportunity: ["Account.Name", "Owner.Name"],
    OpportunityLineItem: ["Product2.Name"],
    Product2: [],
    Pricebook2: [],
    Case: ["Account.Name", "Contact.Name", "Owner.Name"],
    Task: ["Who.Name", "What.Name", "Owner.Name"],
    Event: ["Who.Name", "What.Name", "Owner.Name"],
    User: ["Profile.Name", "UserRole.Name"],
    Campaign: ["Owner.Name"],
    CampaignMember: ["Campaign.Name"],
    Contract: ["Account.Name", "Owner.Name"],
    Order: ["Account.Name", "Owner.Name"],
};

/** A typed read's extra fields: what to select, and how to key the result. */
export interface ExtraFields {
    /** Paths to add to the SELECT: custom fields and relationship paths (standard fields already come with `FIELDS(STANDARD)`). */
    select: string[];
    /** Every field as asked for, read back into `custom`. */
    requested: ApiName[];
    /** Their lowercased paths, sorted and distinct: the check's `fields`. */
    keys: string[];
}

/**
 * Validate a typed read's `fields`.
 * @param fields Field API names or relationship paths, as given.
 * @returns What to select and how to read it back.
 */
export function extraFields(fields: string[]): ExtraFields {
    const select: string[] = [];
    const requested: ApiName[] = [];
    for (const raw of fields) {
        const field = normalizePath(raw, "fields");
        requested.push(field);
        if (field.key.includes(".") || field.key.includes("__")) {
            if (!select.includes(field.name)) select.push(field.name);
        }
    }
    const keys: string[] = [];
    for (const field of requested) keys.push(field.key);
    return { select: select, requested: requested, keys: sortedDistinct(keys) };
}

/**
 * The SOQL of a typed read.
 * @param object The object's API name.
 * @param extra The extra fields.
 * @param where Conditions.
 * @param order An ORDER BY clause's fields, or "".
 * @returns The query.
 */
export function typedSoql(object: string, extra: ExtraFields, where: PreparedCondition[], order: string): string {
    const select = ["FIELDS(STANDARD)"];
    for (const path of RELATIONS[object] ?? []) select.push(path);
    for (const path of extra.select) {
        if (!select.includes(path)) select.push(path);
    }
    let soql = "SELECT " + select.join(", ") + " FROM " + object + whereClause(where);
    if (order.length > 0) soql += " ORDER BY " + order;
    return soql;
}

// The fields asked for, read from a flattened record without regard to case.
export function customOf(fields: Record<string, FieldValue>, requested: ApiName[]): Record<string, FieldValue> {
    const out: Record<string, FieldValue> = {};
    if (requested.length === 0) return out;
    const keys = Object.keys(fields);
    for (const field of requested) {
        let value: FieldValue = null;
        for (const key of keys) {
            if (key.toLowerCase() === field.key) value = fields[key];
        }
        out[field.name] = value;
    }
    return out;
}

/** Conditions of a typed find, and the ids its check sees. */
export class Filters {
    conditions: PreparedCondition[] = [];

    // Exact text.
    text(field: string, value: string | undefined, argument: string): void {
        if (value === undefined) return;
        this.conditions.push({ soql: field + " = " + quote(requireText(value, argument)), key: field.toLowerCase() });
    }

    // Text the field contains.
    contains(field: string, value: string | undefined, argument: string): void {
        if (value === undefined) return;
        this.conditions.push({ soql: field + " LIKE " + containsLiteral(requireText(value, argument)), key: field.toLowerCase() });
    }

    // An id, returned normalized for the check ("" when left out).
    id(field: string, value: string | undefined, argument: string): string {
        if (value === undefined) return "";
        const id = normalizeId(value, argument);
        this.conditions.push({ soql: field + " = '" + id + "'", key: field.toLowerCase() });
        return id;
    }

    flag(field: string, value: boolean | undefined): void {
        if (value === undefined) return;
        this.conditions.push({ soql: field + " = " + (value ? "TRUE" : "FALSE"), key: field.toLowerCase() });
    }

    date(field: string, op: string, value: string | undefined, argument: string): void {
        if (value === undefined) return;
        this.conditions.push({ soql: field + " " + op + " " + requireDate(value, argument), key: field.toLowerCase() });
    }

    dateTime(field: string, op: string, value: string | undefined, argument: string): void {
        if (value === undefined) return;
        this.conditions.push({ soql: field + " " + op + " " + requireDateTime(value, argument), key: field.toLowerCase() });
    }

    atLeast(field: string, value: number | undefined, argument: string): void {
        if (value === undefined) return;
        if (!isFinite(value)) throw validationError("invalid_input", argument + " must be a finite number");
        const text = value.toString();
        if (text.includes("e")) throw validationError("invalid_input", argument + " must be a plain number, got " + text);
        this.conditions.push({ soql: field + " >= " + text, key: field.toLowerCase() });
    }
}

/** A typed write's body and fields, built from typed fields and `custom`. */
export class Writes {
    private typed: FieldWrite[] = [];
    // Every typed field the operation offers, given or not: its check reads
    // their facts from the typed arguments, so `custom` may not write them.
    private owned: string[] = [];

    /**
     * Set a typed field, left out when undefined.
     * @param field Its API name.
     * @param value Its value; null clears it.
     */
    set(field: string, value: FieldValue | undefined): void {
        this.owned.push(field.toLowerCase());
        if (value !== undefined) this.typed.push({ field: field, value: value });
    }

    /**
     * Set a typed id field: normalized, or null to clear it.
     * @param field Its API name.
     * @param value The id.
     * @param argument Argument name for the error message.
     */
    setId(field: string, value: string | null | undefined, argument: string): void {
        this.owned.push(field.toLowerCase());
        if (value === undefined) return;
        this.typed.push({ field: field, value: value === null ? null : normalizeId(value, argument) });
    }

    /**
     * Set a typed date field, `YYYY-MM-DD`.
     * @param field Its API name.
     * @param value The date.
     * @param argument Argument name for the error message.
     */
    setDate(field: string, value: string | undefined, argument: string): void {
        this.owned.push(field.toLowerCase());
        if (value !== undefined) this.typed.push({ field: field, value: requireDate(value, argument) });
    }

    /**
     * Merge `custom` and validate everything.
     * @param custom Other fields by API name, already copied from the caller.
     * @param argument Argument name for error messages.
     * @returns The body and the sorted field keys.
     */
    prepare(custom: FieldWrite[], argument: string): PreparedValues {
        const extra = prepareValues(custom, argument + " custom");
        for (const key of extra.keys) {
            if (key === "ownerid") throw validationError("owner_field", argument + " custom cannot set OwnerId; ownership changes only through changeOwner");
            if (this.owned.includes(key)) throw validationError("typed_field", argument + " custom cannot set " + key + "; use the typed argument");
        }
        const all: FieldWrite[] = [];
        for (const write of this.typed) all.push(write);
        for (const write of custom) all.push(write);
        const prepared = prepareValues(all, argument);
        if (prepared.keys.length === 0) throw validationError("invalid_input", argument + " changes nothing");
        return prepared;
    }
}

function text(fields: Record<string, FieldValue>, name: string): string | null {
    const value = fields[name];
    if (value === undefined || value === null) return null;
    return typeof value === "string" ? value : value.toString();
}

function number(fields: Record<string, FieldValue>, name: string): number | null {
    const value = fields[name];
    return typeof value === "number" ? value : null;
}

function flag(fields: Record<string, FieldValue>, name: string): boolean {
    return fields[name] === true;
}

function base(fields: Record<string, FieldValue>, requested: ApiName[]): StandardRecord {
    return {
        id: text(fields, "Id") ?? "",
        createdDate: text(fields, "CreatedDate"),
        lastModifiedDate: text(fields, "LastModifiedDate"),
        custom: customOf(fields, requested),
    };
}

export function accountFrom(f: Record<string, FieldValue>, requested: ApiName[]): Account {
    const b = base(f, requested);
    return {
        id: b.id, createdDate: b.createdDate, lastModifiedDate: b.lastModifiedDate, custom: b.custom,
        name: text(f, "Name"), type: text(f, "Type"), industry: text(f, "Industry"),
        ownerId: text(f, "OwnerId"), ownerName: text(f, "Owner.Name"),
        parentId: text(f, "ParentId"), parentName: text(f, "Parent.Name"),
        website: text(f, "Website"), phone: text(f, "Phone"),
        billingCity: text(f, "BillingCity"), billingState: text(f, "BillingState"), billingCountry: text(f, "BillingCountry"),
        annualRevenue: number(f, "AnnualRevenue"), numberOfEmployees: number(f, "NumberOfEmployees"),
        accountNumber: text(f, "AccountNumber"), description: text(f, "Description"),
        lastActivityDate: text(f, "LastActivityDate"),
    };
}

export function contactFrom(f: Record<string, FieldValue>, requested: ApiName[]): Contact {
    const b = base(f, requested);
    return {
        id: b.id, createdDate: b.createdDate, lastModifiedDate: b.lastModifiedDate, custom: b.custom,
        accountId: text(f, "AccountId"), accountName: text(f, "Account.Name"),
        firstName: text(f, "FirstName"), lastName: text(f, "LastName"), name: text(f, "Name"),
        email: text(f, "Email"), phone: text(f, "Phone"), mobilePhone: text(f, "MobilePhone"),
        title: text(f, "Title"), department: text(f, "Department"),
        ownerId: text(f, "OwnerId"), ownerName: text(f, "Owner.Name"),
        mailingCity: text(f, "MailingCity"), mailingCountry: text(f, "MailingCountry"),
        leadSource: text(f, "LeadSource"), description: text(f, "Description"),
    };
}

export function leadFrom(f: Record<string, FieldValue>, requested: ApiName[]): Lead {
    const b = base(f, requested);
    return {
        id: b.id, createdDate: b.createdDate, lastModifiedDate: b.lastModifiedDate, custom: b.custom,
        firstName: text(f, "FirstName"), lastName: text(f, "LastName"), name: text(f, "Name"),
        company: text(f, "Company"), email: text(f, "Email"), phone: text(f, "Phone"), title: text(f, "Title"),
        status: text(f, "Status"), leadSource: text(f, "LeadSource"), industry: text(f, "Industry"),
        rating: text(f, "Rating"), ownerId: text(f, "OwnerId"), ownerName: text(f, "Owner.Name"),
        isConverted: flag(f, "IsConverted"), convertedAccountId: text(f, "ConvertedAccountId"),
        convertedContactId: text(f, "ConvertedContactId"), convertedOpportunityId: text(f, "ConvertedOpportunityId"),
        convertedDate: text(f, "ConvertedDate"), description: text(f, "Description"),
    };
}

export function opportunityFrom(f: Record<string, FieldValue>, requested: ApiName[]): Opportunity {
    const b = base(f, requested);
    return {
        id: b.id, createdDate: b.createdDate, lastModifiedDate: b.lastModifiedDate, custom: b.custom,
        name: text(f, "Name"), accountId: text(f, "AccountId"), accountName: text(f, "Account.Name"),
        ownerId: text(f, "OwnerId"), ownerName: text(f, "Owner.Name"), stage: text(f, "StageName"),
        amount: number(f, "Amount"), closeDate: text(f, "CloseDate"), probability: number(f, "Probability"),
        type: text(f, "Type"), leadSource: text(f, "LeadSource"), nextStep: text(f, "NextStep"),
        forecastCategory: text(f, "ForecastCategoryName"), isClosed: flag(f, "IsClosed"), isWon: flag(f, "IsWon"),
        description: text(f, "Description"), lastActivityDate: text(f, "LastActivityDate"),
    };
}

export function lineItemFrom(f: Record<string, FieldValue>, requested: ApiName[]): OpportunityLineItem {
    const b = base(f, requested);
    return {
        id: b.id, createdDate: b.createdDate, lastModifiedDate: b.lastModifiedDate, custom: b.custom,
        opportunityId: text(f, "OpportunityId"), product2Id: text(f, "Product2Id"), productName: text(f, "Product2.Name"),
        pricebookEntryId: text(f, "PricebookEntryId"), quantity: number(f, "Quantity"), unitPrice: number(f, "UnitPrice"),
        listPrice: number(f, "ListPrice"), totalPrice: number(f, "TotalPrice"), discount: number(f, "Discount"),
        serviceDate: text(f, "ServiceDate"), description: text(f, "Description"),
    };
}

export function productFrom(f: Record<string, FieldValue>, requested: ApiName[]): Product {
    const b = base(f, requested);
    return {
        id: b.id, createdDate: b.createdDate, lastModifiedDate: b.lastModifiedDate, custom: b.custom,
        name: text(f, "Name"), productCode: text(f, "ProductCode"), family: text(f, "Family"),
        isActive: flag(f, "IsActive"), description: text(f, "Description"),
    };
}

export function pricebookFrom(f: Record<string, FieldValue>, requested: ApiName[]): Pricebook {
    const b = base(f, requested);
    return {
        id: b.id, createdDate: b.createdDate, lastModifiedDate: b.lastModifiedDate, custom: b.custom,
        name: text(f, "Name"), isActive: flag(f, "IsActive"), isStandard: flag(f, "IsStandard"),
        description: text(f, "Description"),
    };
}

export function caseFrom(f: Record<string, FieldValue>, requested: ApiName[]): Case {
    const b = base(f, requested);
    return {
        id: b.id, createdDate: b.createdDate, lastModifiedDate: b.lastModifiedDate, custom: b.custom,
        caseNumber: text(f, "CaseNumber"), subject: text(f, "Subject"), status: text(f, "Status"),
        priority: text(f, "Priority"), origin: text(f, "Origin"), type: text(f, "Type"), reason: text(f, "Reason"),
        accountId: text(f, "AccountId"), accountName: text(f, "Account.Name"),
        contactId: text(f, "ContactId"), contactName: text(f, "Contact.Name"),
        ownerId: text(f, "OwnerId"), ownerName: text(f, "Owner.Name"),
        isClosed: flag(f, "IsClosed"), closedDate: text(f, "ClosedDate"), description: text(f, "Description"),
    };
}

export function taskFrom(f: Record<string, FieldValue>, requested: ApiName[]): Task {
    const b = base(f, requested);
    return {
        id: b.id, createdDate: b.createdDate, lastModifiedDate: b.lastModifiedDate, custom: b.custom,
        subject: text(f, "Subject"), status: text(f, "Status"), priority: text(f, "Priority"),
        activityDate: text(f, "ActivityDate"), whoId: text(f, "WhoId"), whoName: text(f, "Who.Name"),
        whatId: text(f, "WhatId"), whatName: text(f, "What.Name"), accountId: text(f, "AccountId"),
        ownerId: text(f, "OwnerId"), ownerName: text(f, "Owner.Name"), isClosed: flag(f, "IsClosed"),
        taskSubtype: text(f, "TaskSubtype"), type: text(f, "Type"), description: text(f, "Description"),
    };
}

export function eventFrom(f: Record<string, FieldValue>, requested: ApiName[]): Event {
    const b = base(f, requested);
    return {
        id: b.id, createdDate: b.createdDate, lastModifiedDate: b.lastModifiedDate, custom: b.custom,
        subject: text(f, "Subject"), startDateTime: text(f, "StartDateTime"), endDateTime: text(f, "EndDateTime"),
        isAllDayEvent: flag(f, "IsAllDayEvent"), location: text(f, "Location"),
        whoId: text(f, "WhoId"), whoName: text(f, "Who.Name"), whatId: text(f, "WhatId"), whatName: text(f, "What.Name"),
        accountId: text(f, "AccountId"), ownerId: text(f, "OwnerId"), ownerName: text(f, "Owner.Name"),
        type: text(f, "Type"), description: text(f, "Description"),
    };
}

export function userFrom(f: Record<string, FieldValue>, requested: ApiName[]): User {
    const b = base(f, requested);
    return {
        id: b.id, createdDate: b.createdDate, lastModifiedDate: b.lastModifiedDate, custom: b.custom,
        name: text(f, "Name"), username: text(f, "Username"), email: text(f, "Email"), title: text(f, "Title"),
        department: text(f, "Department"), isActive: flag(f, "IsActive"), profileName: text(f, "Profile.Name"),
        roleName: text(f, "UserRole.Name"), managerId: text(f, "ManagerId"),
    };
}

export function campaignFrom(f: Record<string, FieldValue>, requested: ApiName[]): Campaign {
    const b = base(f, requested);
    return {
        id: b.id, createdDate: b.createdDate, lastModifiedDate: b.lastModifiedDate, custom: b.custom,
        name: text(f, "Name"), type: text(f, "Type"), status: text(f, "Status"), startDate: text(f, "StartDate"),
        endDate: text(f, "EndDate"), isActive: flag(f, "IsActive"), budgetedCost: number(f, "BudgetedCost"),
        actualCost: number(f, "ActualCost"), expectedRevenue: number(f, "ExpectedRevenue"),
        numberOfLeads: number(f, "NumberOfLeads"), numberOfContacts: number(f, "NumberOfContacts"),
        numberOfOpportunities: number(f, "NumberOfOpportunities"), ownerId: text(f, "OwnerId"),
        ownerName: text(f, "Owner.Name"), parentId: text(f, "ParentId"),
    };
}

export function campaignMemberFrom(f: Record<string, FieldValue>, requested: ApiName[]): CampaignMember {
    const b = base(f, requested);
    return {
        id: b.id, createdDate: b.createdDate, lastModifiedDate: b.lastModifiedDate, custom: b.custom,
        campaignId: text(f, "CampaignId"), campaignName: text(f, "Campaign.Name"), leadId: text(f, "LeadId"),
        contactId: text(f, "ContactId"), status: text(f, "Status"), hasResponded: flag(f, "HasResponded"),
        firstRespondedDate: text(f, "FirstRespondedDate"),
    };
}

export function contractFrom(f: Record<string, FieldValue>, requested: ApiName[]): Contract {
    const b = base(f, requested);
    return {
        id: b.id, createdDate: b.createdDate, lastModifiedDate: b.lastModifiedDate, custom: b.custom,
        accountId: text(f, "AccountId"), accountName: text(f, "Account.Name"), contractNumber: text(f, "ContractNumber"),
        status: text(f, "Status"), startDate: text(f, "StartDate"), endDate: text(f, "EndDate"),
        contractTerm: number(f, "ContractTerm"), ownerId: text(f, "OwnerId"), ownerName: text(f, "Owner.Name"),
        description: text(f, "Description"),
    };
}

export function orderFrom(f: Record<string, FieldValue>, requested: ApiName[]): Order {
    const b = base(f, requested);
    return {
        id: b.id, createdDate: b.createdDate, lastModifiedDate: b.lastModifiedDate, custom: b.custom,
        accountId: text(f, "AccountId"), accountName: text(f, "Account.Name"), orderNumber: text(f, "OrderNumber"),
        status: text(f, "Status"), effectiveDate: text(f, "EffectiveDate"), endDate: text(f, "EndDate"),
        totalAmount: number(f, "TotalAmount"), contractId: text(f, "ContractId"), ownerId: text(f, "OwnerId"),
        ownerName: text(f, "Owner.Name"), type: text(f, "Type"), description: text(f, "Description"),
    };
}
