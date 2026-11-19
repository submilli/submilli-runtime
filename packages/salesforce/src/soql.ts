// SOQL and SOSL written by the package. Programs describe a query with typed
// conditions; names are validated as API names and every value is encoded as a
// SOQL literal here, so no program text reaches a query unescaped. Internal:
// `src/lib.ts` re-exports the input types.

import { validationError } from "./errors";
import { isDate, isDateTime, isDigits, normalizeName, normalizePath, sortedDistinct } from "./request";

const MAX_QUERY_LIMIT = 50000;
const MAX_AGGREGATE_LIMIT = 2000;
const MAX_IN_VALUES = 1000;
const MAX_SEARCH_LIMIT = 2000;

/** A comparison. `like` matches a pattern where `%` is any text and `_` one character; `in`/`not in` match a list; `includes`/`excludes` test a multi-select picklist for any of a list's values. */
export type ConditionOperator = "=" | "!=" | "<" | "<=" | ">" | ">=" | "like" | "in" | "not in" | "includes" | "excludes";

/**
 * One condition. Conditions in a list are combined with AND. Give exactly one
 * of `value`, `values`, `date`, `dateTime` and `dateLiteral`:
 *
 * | Field | For | SOQL |
 * | --- | --- | --- |
 * | `value` | text, numbers, checkboxes, null (with `=` and `!=`) | `'Acme'`, `1200`, `TRUE`, `NULL` |
 * | `values` | `in`, `not in`, `includes`, `excludes` | `('Prospecting', 'Qualification')` |
 * | `date` | a date field, `YYYY-MM-DD` | `2026-11-01` |
 * | `dateTime` | a date-time field, with a zone | `2026-11-01T09:30:00Z` |
 * | `dateLiteral` | a relative date such as `THIS_QUARTER` or `LAST_N_DAYS:30` | `THIS_QUARTER` |
 */
export interface Condition {
    /** Field API name or relationship path, such as `StageName` or `Account.Industry`. */
    field: string;
    /** The comparison. */
    op: ConditionOperator;
    /** Text, a number, a checkbox, or null for an empty field. */
    value?: string | number | boolean | null;
    /** The list for `in`, `not in`, `includes` and `excludes`. */
    values?: (string | number)[];
    /** A date, `YYYY-MM-DD`, for a date field. */
    date?: string;
    /** A date-time with a zone, such as `2026-11-01T09:30:00Z`, for a date-time field. */
    dateTime?: string;
    /** A relative date such as `TODAY`, `THIS_QUARTER` or `LAST_N_DAYS:30`. */
    dateLiteral?: string;
}

/** One sort key. */
export interface OrderBy {
    /** Field API name or relationship path. */
    field: string;
    /** Default `asc`. */
    direction?: "asc" | "desc";
    /** Where empty values sort; Salesforce's default is first for `asc`, last for `desc`. */
    nulls?: "first" | "last";
}

/** A query of one object's records. */
export interface QuerySpec {
    /** Object API name, such as `Opportunity` or `Invoice__c`. */
    object: string;
    /** Fields to read: API names or relationship paths such as `Account.Name`. `Id` is always read. */
    fields: string[];
    /** Conditions, combined with AND. */
    where?: Condition[];
    /** Sort keys, first one first. */
    orderBy?: OrderBy[];
    /** The most records the query returns across all its pages, 1 to 50000; default no limit. */
    limit?: number;
}

/** An aggregate function. `count` without a field counts records. */
export type MeasureFunction = "count" | "count_distinct" | "sum" | "avg" | "min" | "max";

/** One aggregate. Its result is keyed by its label: `count()`, or the function and field as given, such as `sum(Amount)`. */
export interface Measure {
    /** The function. */
    fn: MeasureFunction;
    /** The field aggregated; required except for `count`. */
    field?: string;
}

/** A date grouping, Salesforce's date functions: `calendar_month(CloseDate)` groups by month 1–12. */
export type DateGroup = "calendar_month" | "calendar_quarter" | "calendar_year" | "fiscal_quarter" | "fiscal_year" | "day_only" | "week_in_year";

/** One grouping. Its result is keyed by its label: the field as given, or the date function and field, such as `calendar_month(CloseDate)`. */
export interface GroupKey {
    /** Field API name or relationship path, such as `StageName` or `Owner.Name`. */
    field: string;
    /** Group a date or date-time field by this part of it. */
    date?: DateGroup;
}

/** A sort key of an aggregate: a group or measure label, such as `sum(Amount)`. */
export interface AggregateOrder {
    /** A group label, such as `StageName`, or a measure label, such as `sum(Amount)`. */
    by: string;
    /** Default `asc`. */
    direction?: "asc" | "desc";
}

/** An aggregate query: measures over records, optionally grouped. */
export interface AggregateSpec {
    /** Object API name. */
    object: string;
    /** Groupings; leave out for one row over every matching record. */
    groupBy?: GroupKey[];
    /** At least one measure. */
    measures: Measure[];
    /** Conditions on records, combined with AND. */
    where?: Condition[];
    /** Sort keys, by label. */
    orderBy?: AggregateOrder[];
    /** The most rows, 1 to 2000 (default 2000; Salesforce returns at most 2000 groups). */
    limit?: number;
}

/** One object a search covers. */
export interface SearchObject {
    /** Object API name, such as `Account`. */
    object: string;
    /** Fields to return for its matches; `Id` is always returned. */
    fields?: string[];
    /** The most matches of this object, 1 to 2000. */
    limit?: number;
}

/** A full-text search across objects. */
export interface SearchSpec {
    /** Words to find, at least two characters. SOSL's reserved characters are searched as given; the words AND, OR and NOT act as operators. */
    text: string;
    /** The objects to search, at least one. */
    objects: SearchObject[];
    /** The most matches across all objects, 1 to 2000 (default 200). */
    limit?: number;
}

/** A query written by the package, with what its check sees. */
export interface PreparedQuery {
    soql: string;
    /** The object's lowercased API name. */
    object: string;
    /** Every field the query reads, filters or sorts on: lowercased, sorted, distinct. */
    fields: string[];
}

/** An aggregate query, with the labels of its result columns. */
export interface PreparedAggregate extends PreparedQuery {
    /** Group labels, in order: the result's `g0`, `g1`, … */
    groups: string[];
    /** Measure labels, in order: the result's `m0`, `m1`, … */
    measures: string[];
}

/** A search request body, with what its check sees. */
export interface PreparedSearch {
    body: Record<string, unknown>;
    /** Lowercased object names, sorted and distinct. */
    objects: string[];
    /** Lowercased names of the fields returned, sorted and distinct. */
    fields: string[];
}

/** A condition rendered as SOQL, with the key of its field. */
export interface PreparedCondition {
    soql: string;
    key: string;
}

/**
 * Write a query's SOQL.
 * @param spec The query as given.
 * @returns The SOQL and the check's object and fields.
 */
export function buildQuery(spec: QuerySpec): PreparedQuery {
    const object = normalizeName(spec.object, "object");
    if (spec.fields.length === 0) throw validationError("invalid_input", "fields cannot be empty");
    const selected: string[] = ["Id"];
    const selectedKeys: string[] = ["id"];
    const keys: string[] = [];
    for (const raw of spec.fields) {
        const field = normalizePath(raw, "fields");
        keys.push(field.key);
        if (selectedKeys.includes(field.key)) continue;
        selected.push(field.name);
        selectedKeys.push(field.key);
    }
    const where = prepareConditions(spec.where ?? []);
    for (const condition of where) keys.push(condition.key);
    const order: string[] = [];
    for (const sort of spec.orderBy ?? []) {
        const field = normalizePath(sort.field, "orderBy field");
        keys.push(field.key);
        order.push(field.name + orderSuffix(sort.direction, sort.nulls));
    }
    let soql = "SELECT " + selected.join(", ") + " FROM " + object.name + whereClause(where);
    if (order.length > 0) soql += " ORDER BY " + order.join(", ");
    if (spec.limit !== undefined) soql += " LIMIT " + limitOf(spec.limit, MAX_QUERY_LIMIT).toString();
    return { soql: soql, object: object.key, fields: sortedDistinct(keys) };
}

/**
 * Write an aggregate query's SOQL. Groups are aliased `g0`, `g1`, … and
 * measures `m0`, `m1`, …, so their results map back to their labels.
 * @param spec The aggregate as given.
 * @returns The SOQL, the check's object and fields, and the labels.
 */
export function buildAggregate(spec: AggregateSpec): PreparedAggregate {
    const object = normalizeName(spec.object, "object");
    if (spec.measures.length === 0) throw validationError("invalid_input", "measures cannot be empty");
    const keys: string[] = [];
    const select: string[] = [];
    const groupExpressions: string[] = [];
    const groups: string[] = [];
    const measures: string[] = [];
    const measureExpressions: string[] = [];
    for (const group of spec.groupBy ?? []) {
        const field = normalizePath(group.field, "groupBy field");
        keys.push(field.key);
        const expression = group.date === undefined ? field.name : dateFunction(group.date) + "(" + field.name + ")";
        const label = group.date === undefined ? field.name : group.date + "(" + field.name + ")";
        if (groups.includes(label)) throw validationError("invalid_input", "groupBy repeats " + label);
        select.push(expression + " g" + groups.length.toString());
        groupExpressions.push(expression);
        groups.push(label);
    }
    for (const measure of spec.measures) {
        const fn = measureFunction(measure.fn);
        let argument = "Id";
        let label = "count()";
        if (measure.field !== undefined) {
            const field = normalizePath(measure.field, "measure field");
            keys.push(field.key);
            argument = field.name;
            label = measure.fn + "(" + field.name + ")";
        } else if (measure.fn !== "count") {
            throw validationError("invalid_input", measure.fn + " needs a field");
        }
        if (measures.includes(label)) throw validationError("invalid_input", "measures repeat " + label);
        select.push(fn + "(" + argument + ") m" + measures.length.toString());
        measures.push(label);
        measureExpressions.push(fn + "(" + argument + ")");
    }
    const where = prepareConditions(spec.where ?? []);
    for (const condition of where) keys.push(condition.key);
    const order: string[] = [];
    for (const sort of spec.orderBy ?? []) {
        const label = sort.by.trim();
        const group = groups.indexOf(label);
        const measure = measures.indexOf(label);
        if (group < 0 && measure < 0) {
            throw validationError("invalid_input", "orderBy " + JSON.stringify(sort.by) + " must be a group or measure label: " + groups.concat(measures).join(", "));
        }
        const expression = group >= 0 ? groupExpressions[group] : measureExpressions[measure];
        order.push(expression + orderSuffix(sort.direction, undefined));
    }
    let soql = "SELECT " + select.join(", ") + " FROM " + object.name + whereClause(where);
    if (groupExpressions.length > 0) soql += " GROUP BY " + groupExpressions.join(", ");
    if (order.length > 0) soql += " ORDER BY " + order.join(", ");
    const limit = spec.limit === undefined ? MAX_AGGREGATE_LIMIT : limitOf(spec.limit, MAX_AGGREGATE_LIMIT);
    // Salesforce refuses LIMIT on an ungrouped aggregate, which has one row anyway.
    if (groupExpressions.length > 0) soql += " LIMIT " + limit.toString();
    return { soql: soql, object: object.key, fields: sortedDistinct(keys), groups: groups, measures: measures };
}

/**
 * Build a `parameterizedSearch` body. SOSL's reserved characters in the text are
 * escaped; its words `AND`, `OR` and `NOT` still act as operators.
 * @param spec The search as given.
 * @returns The body and the check's objects and fields.
 */
export function buildSearch(spec: SearchSpec): PreparedSearch {
    const text = spec.text.trim();
    if (text.length < 2) throw validationError("invalid_input", "text must have at least two characters");
    if (spec.objects.length === 0) throw validationError("invalid_input", "objects cannot be empty");
    const objects: string[] = [];
    const fieldKeys: string[] = [];
    const sobjects: Record<string, unknown>[] = [];
    for (const target of spec.objects) {
        const object = normalizeName(target.object, "objects object");
        if (objects.includes(object.key)) throw validationError("invalid_input", "objects repeats " + object.name);
        objects.push(object.key);
        const fields: string[] = ["Id"];
        const seen: string[] = ["id"];
        for (const raw of target.fields ?? []) {
            const field = normalizePath(raw, "objects fields");
            fieldKeys.push(field.key);
            if (seen.includes(field.key)) continue;
            fields.push(field.name);
            seen.push(field.key);
        }
        const entry: Record<string, unknown> = { name: object.name, fields: fields };
        if (target.limit !== undefined) entry["limit"] = limitOf(target.limit, MAX_SEARCH_LIMIT);
        sobjects.push(entry);
    }
    const overall = spec.limit === undefined ? 200 : limitOf(spec.limit, MAX_SEARCH_LIMIT);
    return {
        body: { q: escapeSosl(text), in: "ALL", sobjects: sobjects, overallLimit: overall },
        objects: sortedDistinct(objects),
        fields: sortedDistinct(fieldKeys),
    };
}

/**
 * Render conditions, validating each.
 * @param conditions The conditions as given.
 * @returns Each condition's SOQL and field key.
 */
export function prepareConditions(conditions: Condition[]): PreparedCondition[] {
    const out: PreparedCondition[] = [];
    for (const condition of conditions) out.push(prepareCondition(condition));
    return out;
}

/**
 * Render one condition.
 * @param condition The condition as given.
 * @returns Its SOQL and field key.
 */
export function prepareCondition(condition: Condition): PreparedCondition {
    const field = normalizePath(condition.field, "where field");
    const op = condition.op;
    const forms = (condition.value !== undefined ? 1 : 0) + (condition.values !== undefined ? 1 : 0)
        + (condition.date !== undefined ? 1 : 0) + (condition.dateTime !== undefined ? 1 : 0)
        + (condition.dateLiteral !== undefined ? 1 : 0);
    const name = field.name;
    if (forms !== 1) {
        throw validationError("invalid_condition", "condition on " + name + " must give exactly one of value, values, date, dateTime and dateLiteral");
    }
    const list = op === "in" || op === "not in" || op === "includes" || op === "excludes";
    const values = condition.values;
    if (list !== (values !== undefined)) {
        throw validationError("invalid_condition", "condition on " + name + ": " + op + (list ? " takes values" : " takes one value, not values"));
    }
    if (values !== undefined) {
        if (values.length === 0 || values.length > MAX_IN_VALUES) {
            throw validationError("invalid_condition", "condition on " + name + ": values must have 1 to " + MAX_IN_VALUES.toString() + " entries");
        }
        const literals: string[] = [];
        for (const value of values) literals.push(scalarLiteral(value, name));
        return { soql: name + " " + op.toUpperCase() + " (" + literals.join(", ") + ")", key: field.key };
    }
    const operator = op === "like" ? "LIKE" : op;
    if (!["=", "!=", "<", "<=", ">", ">=", "like"].includes(op)) {
        throw validationError("invalid_condition", "condition on " + name + ": unknown operator " + JSON.stringify(op));
    }
    const date = condition.date;
    const dateTime = condition.dateTime;
    const dateLiteral = condition.dateLiteral;
    let literal = "";
    if (date !== undefined) {
        if (!isDate(date)) throw validationError("invalid_date", "condition on " + name + ": date must be YYYY-MM-DD, got " + JSON.stringify(date));
        literal = date;
    } else if (dateTime !== undefined) {
        if (!isDateTime(dateTime)) {
            throw validationError("invalid_date", "condition on " + name + ": dateTime must be like 2026-11-01T09:30:00Z, got " + JSON.stringify(dateTime));
        }
        literal = dateTime;
    } else if (dateLiteral !== undefined) {
        literal = relativeDate(dateLiteral, name);
    } else {
        const value = condition.value;
        if (value === null) {
            if (op !== "=" && op !== "!=") throw validationError("invalid_condition", "condition on " + name + ": null compares only with = or !=");
            literal = "NULL";
        } else if (typeof value === "boolean") {
            if (op !== "=" && op !== "!=") throw validationError("invalid_condition", "condition on " + name + ": a checkbox compares only with = or !=");
            literal = value ? "TRUE" : "FALSE";
        } else if (value !== undefined) {
            if (op === "like" && typeof value !== "string") throw validationError("invalid_condition", "condition on " + name + ": like takes text");
            literal = scalarLiteral(value, name);
        }
    }
    if (op === "like" && (date !== undefined || dateTime !== undefined || dateLiteral !== undefined)) {
        throw validationError("invalid_condition", "condition on " + name + ": like takes text");
    }
    return { soql: name + " " + operator + " " + literal, key: field.key };
}

/**
 * A text or number as a SOQL literal.
 * @param value The value.
 * @param field The field it is compared with, for the error message.
 * @returns The literal.
 */
export function scalarLiteral(value: string | number, field: string): string {
    if (typeof value === "string") return quote(value);
    const text = value.toString();
    if (!isFinite(value) || text.includes("e")) {
        throw validationError("invalid_condition", "condition on " + field + ": " + text + " is not a number SOQL can compare");
    }
    return text;
}

/**
 * Quote text as a SOQL string literal, escaping backslashes, quotes and
 * control characters.
 * @param text The text.
 * @returns The quoted literal.
 */
export function quote(text: string): string {
    return "'" + escapeText(text, false) + "'";
}

/**
 * A `LIKE` literal matching text that contains `text`, with `%` and `_` in it
 * matched literally.
 * @param text The text to find.
 * @returns The quoted pattern.
 */
export function containsLiteral(text: string): string {
    return "'%" + escapeText(text, true) + "%'";
}

function escapeText(text: string, wildcards: boolean): string {
    let out = "";
    for (let i = 0; i < text.length; i += 1) {
        const char = text.charAt(i);
        if (char === "\\") out += "\\\\";
        else if (char === "'") out += "\\'";
        else if (char === "\"") out += "\\\"";
        else if (char === "\n") out += "\\n";
        else if (char === "\r") out += "\\r";
        else if (char === "\t") out += "\\t";
        else if (char === "\b") out += "\\b";
        else if (char === "\f") out += "\\f";
        else if (wildcards && (char === "%" || char === "_")) out += "\\" + char;
        else out += char;
    }
    return out;
}

const SOSL_RESERVED = "?&|!{}[]()^~*:\\\"'+-";

/**
 * Escape SOSL's reserved characters, so text is searched as written.
 * @param text The text.
 * @returns The escaped text.
 */
export function escapeSosl(text: string): string {
    let out = "";
    for (let i = 0; i < text.length; i += 1) {
        const char = text.charAt(i);
        if (SOSL_RESERVED.includes(char)) out += "\\";
        out += char;
    }
    return out;
}

// ` WHERE a AND b`, or "" for no conditions.
export function whereClause(conditions: PreparedCondition[]): string {
    if (conditions.length === 0) return "";
    const parts: string[] = [];
    for (const condition of conditions) parts.push(condition.soql);
    return " WHERE " + parts.join(" AND ");
}

const RELATIVE_DATES = [
    "YESTERDAY", "TODAY", "TOMORROW", "LAST_WEEK", "THIS_WEEK", "NEXT_WEEK", "LAST_MONTH", "THIS_MONTH",
    "NEXT_MONTH", "LAST_90_DAYS", "NEXT_90_DAYS", "THIS_QUARTER", "LAST_QUARTER", "NEXT_QUARTER",
    "THIS_YEAR", "LAST_YEAR", "NEXT_YEAR", "THIS_FISCAL_QUARTER", "LAST_FISCAL_QUARTER",
    "NEXT_FISCAL_QUARTER", "THIS_FISCAL_YEAR", "LAST_FISCAL_YEAR", "NEXT_FISCAL_YEAR",
];

const RELATIVE_DATES_N = [
    "LAST_N_DAYS", "NEXT_N_DAYS", "N_DAYS_AGO", "LAST_N_WEEKS", "NEXT_N_WEEKS", "N_WEEKS_AGO",
    "LAST_N_MONTHS", "NEXT_N_MONTHS", "N_MONTHS_AGO", "LAST_N_QUARTERS", "NEXT_N_QUARTERS",
    "N_QUARTERS_AGO", "LAST_N_YEARS", "NEXT_N_YEARS", "N_YEARS_AGO", "LAST_N_FISCAL_QUARTERS",
    "NEXT_N_FISCAL_QUARTERS", "N_FISCAL_QUARTERS_AGO", "LAST_N_FISCAL_YEARS", "NEXT_N_FISCAL_YEARS",
    "N_FISCAL_YEARS_AGO",
];

/**
 * Validate a relative date literal, such as `THIS_QUARTER` or `LAST_N_DAYS:30`.
 * @param value The literal as given.
 * @param field The field it is compared with, for the error message.
 * @returns The literal, uppercased.
 */
export function relativeDate(value: string, field: string): string {
    const text = value.trim().toUpperCase();
    if (RELATIVE_DATES.includes(text)) return text;
    const colon = text.indexOf(":");
    if (colon > 0 && RELATIVE_DATES_N.includes(text.slice(0, colon))) {
        const n = text.slice(colon + 1);
        if (isDigits(n) && n.length <= 4) return text;
    }
    throw validationError(
        "invalid_date",
        "condition on " + field + ": dateLiteral must be a Salesforce date literal such as THIS_QUARTER or LAST_N_DAYS:30, got " + JSON.stringify(value),
    );
}

function dateFunction(group: DateGroup): string {
    if (group === "calendar_month") return "CALENDAR_MONTH";
    if (group === "calendar_quarter") return "CALENDAR_QUARTER";
    if (group === "calendar_year") return "CALENDAR_YEAR";
    if (group === "fiscal_quarter") return "FISCAL_QUARTER";
    if (group === "fiscal_year") return "FISCAL_YEAR";
    if (group === "day_only") return "DAY_ONLY";
    if (group === "week_in_year") return "WEEK_IN_YEAR";
    throw validationError("invalid_input", "unknown date grouping " + JSON.stringify(group));
}

function measureFunction(fn: MeasureFunction): string {
    if (fn === "count") return "COUNT";
    if (fn === "count_distinct") return "COUNT_DISTINCT";
    if (fn === "sum") return "SUM";
    if (fn === "avg") return "AVG";
    if (fn === "min") return "MIN";
    if (fn === "max") return "MAX";
    throw validationError("invalid_input", "unknown measure " + JSON.stringify(fn));
}

function orderSuffix(direction: "asc" | "desc" | undefined, nulls: "first" | "last" | undefined): string {
    let out = "";
    if (direction === "desc") out += " DESC";
    else if (direction === "asc" || direction === undefined) out += " ASC";
    else throw validationError("invalid_input", "direction must be asc or desc");
    if (nulls === "first") out += " NULLS FIRST";
    else if (nulls === "last") out += " NULLS LAST";
    else if (nulls !== undefined) throw validationError("invalid_input", "nulls must be first or last");
    return out;
}

function limitOf(limit: number, max: number): number {
    if (!Number.isInteger(limit) || limit < 1 || limit > max) {
        throw validationError("invalid_input", "limit must be an integer from 1 to " + max.toString() + ", got " + limit.toString());
    }
    return limit;
}
