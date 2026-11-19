// Reports and dashboards: the result types, report filters, and the flattening
// of Salesforce's report format (groupings and fact maps) into rows and totals.
// Internal: `src/lib.ts` re-exports the types.

import { validationError } from "./errors";
import { FieldValue } from "./request";
import { arrayAt, numberAt, objectAt, textAt } from "./response";

/** A report filter's operator. */
export type ReportOperator =
    | "equals" | "notEqual" | "lessThan" | "greaterThan" | "lessOrEqual" | "greaterOrEqual"
    | "contains" | "notContain" | "startsWith" | "includes" | "excludes";

/** A filter added to a report's own filters for one run. */
export interface ReportFilter {
    /** The column's API name, as `describeReport` lists it, such as `STAGE_NAME` or `Account.Industry`. */
    column: string;
    /** How the column compares with `value`. */
    operator: ReportOperator;
    /** The value, as the report UI would take it, such as `Prospecting` or `2026-01-01`; several values comma-separated. */
    value: string;
}

/** A filter a report has saved. */
export interface SavedFilter {
    /** The column's API name. */
    column: string;
    /** Its operator, such as `equals`. */
    operator: string;
    /** Its value. */
    value: string;
}

/** Options of `runReport`. */
export interface RunReportOptions {
    /** Filters added, with AND, to the report's own for this run. */
    filters?: ReportFilter[];
    /** False for totals only, without detail rows; default true. */
    includeDetails?: boolean;
    /** True to also return Salesforce's own result, as JSON text, in `raw`. */
    raw?: boolean;
}

/** A report column, grouping or aggregate. */
export interface ReportColumn {
    /** API name: the key in rows and totals, such as `AMOUNT` or `s!AMOUNT`. */
    name: string;
    /** Label, such as `Amount` or `Sum of Amount`. */
    label: string;
    /** Such as `string`, `currency`, `date`, `picklist` or `int`. */
    dataType: string;
}

/** One subtotal or the grand total of a report. */
export interface ReportTotal {
    /** The group values it totals, by grouping name; empty for the grand total. */
    groups: Record<string, FieldValue>;
    /** The aggregates by name, such as `{ RowCount: 12, "s!AMOUNT": 480000 }`. */
    values: Record<string, FieldValue>;
}

/** A report's result, flattened. */
export interface ReportResult {
    /** Report id. */
    reportId: string;
    /** Report name. */
    name: string;
    /** `TABULAR`, `SUMMARY` or `MATRIX`. */
    format: string;
    /** The detail columns, in order. */
    columns: ReportColumn[];
    /** The groupings, rows (down) first, then columns (across) of a matrix. */
    groupings: ReportColumn[];
    /** The aggregates each total has. */
    aggregates: ReportColumn[];
    /** One row per detail record by column name, its group values included. Numbers, dates and booleans are values; other cells are their displayed text. */
    rows: Record<string, FieldValue>[];
    /** Every subtotal, then the grand total last. */
    totals: ReportTotal[];
    /** True when Salesforce cut the detail rows at 2000; `exportQuery` reads all of them. */
    truncated: boolean;
    /** Salesforce's own result as JSON text, when asked for with `raw`; else null. */
    raw: string | null;
}

/** A report, as listed. */
export interface ReportSummary {
    /** Report id. */
    id: string;
    /** `Name`. */
    name: string | null;
    /** `DeveloperName`. */
    developerName: string | null;
    /** `FolderName`. */
    folderName: string | null;
    /** `Format`: `Tabular`, `Summary` or `Matrix`. */
    format: string | null;
    /** `Description`. */
    description: string | null;
    /** `LastRunDate`. */
    lastRunDate: string | null;
}

/** A report's definition. */
export interface ReportDescription {
    /** Report id. */
    id: string;
    /** Report name. */
    name: string;
    /** `TABULAR`, `SUMMARY` or `MATRIX`. */
    format: string;
    /** The report type's API name, such as `Opportunity`. */
    reportType: string;
    /** The detail columns. */
    columns: ReportColumn[];
    /** The groupings. */
    groupings: ReportColumn[];
    /** The aggregates. */
    aggregates: ReportColumn[];
    /** Its saved filters. */
    filters: SavedFilter[];
    /** How the filters combine, such as `1 AND (2 OR 3)`; null for all with AND. */
    booleanFilter: string | null;
}

/** Options of `listReports` and `listDashboards`. */
export interface ListOptions {
    /** Only those whose name (a dashboard's title) contains this text. */
    name?: string;
    /** Only those in this folder, by its name. */
    folder?: string;
    /** Records per page, 200 to 2000; default 200. */
    pageSize?: number;
    /** A page's `nextCursor`, for the next page. */
    cursor?: string;
}

/** A dashboard, as listed. */
export interface DashboardSummary {
    /** Dashboard id. */
    id: string;
    /** `Title`. */
    title: string | null;
    /** `DeveloperName`. */
    developerName: string | null;
    /** `FolderName`. */
    folderName: string | null;
    /** `Description`. */
    description: string | null;
    /** `LastModifiedDate`. */
    lastModifiedDate: string | null;
}

/** A dashboard and its components' latest data. */
export interface Dashboard {
    /** Dashboard id. */
    id: string;
    /** Its name. */
    name: string;
    /** The user whose access the data reflects. */
    runningUserId: string | null;
    /** Its components. */
    components: DashboardComponent[];
}

/** One dashboard component and the totals it shows. */
export interface DashboardComponent {
    /** Component id. */
    id: string;
    /** Header. */
    header: string | null;
    /** Title. */
    title: string | null;
    /** The source report. */
    reportId: string | null;
    /** Such as `Bar`, `Donut`, `Metric` or `Table`. */
    type: string | null;
    /** When its data was refreshed. */
    refreshDate: string | null;
    /** The groupings its totals are by. */
    groupings: ReportColumn[];
    /** The aggregates its totals have. */
    aggregates: ReportColumn[];
    /** Its subtotals and grand total; empty when it has no data yet. */
    totals: ReportTotal[];
}

const OPERATORS: string[] = [
    "equals", "notEqual", "lessThan", "greaterThan", "lessOrEqual", "greaterOrEqual",
    "contains", "notContain", "startsWith", "includes", "excludes",
];
const NUMBER_TYPES: string[] = ["currency", "double", "int", "percent", "number"];
const VALUE_TYPES: string[] = ["date", "datetime", "time", "boolean"];
const MAX_COLUMN = 255;

/**
 * Validate a run's added filter.
 * @param filter The filter, already copied from the caller.
 * @returns The filter as the Analytics API takes it.
 */
export function reportFilter(filter: ReportFilter): Record<string, unknown> {
    const column = filter.column.trim();
    let ok = column.length > 0 && column.length <= MAX_COLUMN;
    for (let i = 0; ok && i < column.length; i += 1) {
        const char = column.charAt(i);
        const code = column.charCodeAt(i);
        ok = (code >= 65 && code <= 90) || (code >= 97 && code <= 122) || (code >= 48 && code <= 57) || char === "_" || char === "." || char === "$";
    }
    if (!ok) throw validationError("invalid_input", "a filter's column must be a column API name, got " + JSON.stringify(filter.column));
    if (!OPERATORS.includes(filter.operator)) {
        throw validationError("invalid_input", "a filter's operator must be one of " + OPERATORS.join(", ") + ", got " + JSON.stringify(filter.operator));
    }
    return { column: column, operator: filter.operator, value: filter.value };
}

/**
 * A report's metadata with filters added, for `POST analytics/reports/{id}`.
 * @param metadata The report's `reportMetadata`, from its describe.
 * @param added The filters to add.
 * @returns The metadata to run.
 */
export function withFilters(metadata: Record<string, unknown>, added: Record<string, unknown>[]): Record<string, unknown> {
    const out: Record<string, unknown> = {};
    for (const key of Object.keys(metadata)) out[key] = metadata[key];
    const filters: unknown[] = [];
    for (const filter of arrayAt(metadata, "reportFilters")) filters.push(filter);
    const existing = filters.length;
    for (const filter of added) filters.push(filter);
    out["reportFilters"] = filters;
    const logic = textAt(metadata, "reportBooleanFilter");
    if (logic !== null && logic.trim().length > 0) {
        let combined = "(" + logic + ")";
        for (let i = 0; i < added.length; i += 1) combined += " AND " + (existing + i + 1).toString();
        out["reportBooleanFilter"] = combined;
    }
    return out;
}

/**
 * Flatten a report result.
 * @param result Salesforce's report result (or a dashboard component's).
 * @param details Whether to read detail rows.
 * @returns The columns, rows and totals.
 */
export function reportFrom(result: Record<string, unknown>, details: boolean): ReportResult {
    const metadata = objectAt(result, "reportMetadata");
    const extended = objectAt(result, "reportExtendedMetadata");
    const columns = columnsOf(arrayAt(metadata, "detailColumns"), objectAt(extended, "detailColumnInfo"));
    const down = groupingColumns(arrayAt(metadata, "groupingsDown"), objectAt(extended, "groupingColumnInfo"));
    const across = groupingColumns(arrayAt(metadata, "groupingsAcross"), objectAt(extended, "groupingColumnInfo"));
    const aggregates = columnsOf(arrayAt(metadata, "aggregates"), objectAt(extended, "aggregateColumnInfo"));
    const downNodes = groupNodes(objectAt(result, "groupingsDown"), down);
    const acrossNodes = groupNodes(objectAt(result, "groupingsAcross"), across);
    const factMap = objectAt(result, "factMap");
    const rows: Record<string, FieldValue>[] = [];
    const subtotals: ReportTotal[] = [];
    let grand: ReportTotal | null = null;
    for (const d of downNodes) {
        for (const a of acrossNodes) {
            const fact = objectAt(factMap, d.key + "!" + a.key);
            if (Object.keys(fact).length === 0) continue;
            const groups: Record<string, FieldValue> = {};
            for (const key of Object.keys(d.groups)) groups[key] = d.groups[key];
            for (const key of Object.keys(a.groups)) groups[key] = a.groups[key];
            const values: Record<string, FieldValue> = {};
            const cells = arrayAt(fact, "aggregates");
            for (let i = 0; i < aggregates.length && i < cells.length; i += 1) values[aggregates[i].name] = aggregateValue(cells[i]);
            const total: ReportTotal = { groups: groups, values: values };
            if (d.key === "T" && a.key === "T") grand = total;
            else subtotals.push(total);
            if (!details || !d.leaf || !a.leaf) continue;
            for (const row of arrayAt(fact, "rows")) {
                const flat: Record<string, FieldValue> = {};
                for (const key of Object.keys(groups)) flat[key] = groups[key];
                const dataCells = arrayAt(row, "dataCells");
                for (let i = 0; i < columns.length && i < dataCells.length; i += 1) {
                    flat[columns[i].name] = cellValue(dataCells[i], columns[i].dataType);
                }
                rows.push(flat);
            }
        }
    }
    if (grand !== null) subtotals.push(grand);
    const groupings: ReportColumn[] = [];
    for (const column of down) groupings.push(column);
    for (const column of across) groupings.push(column);
    const attributes = objectAt(result, "attributes");
    return {
        reportId: textAt(metadata, "id") ?? textAt(attributes, "reportId") ?? "",
        name: textAt(metadata, "name") ?? textAt(attributes, "reportName") ?? "",
        format: textAt(metadata, "reportFormat") ?? "",
        columns: columns,
        groupings: groupings,
        aggregates: aggregates,
        rows: rows,
        totals: subtotals,
        truncated: result["allData"] === false,
        raw: null,
    };
}

/**
 * A report's definition, from its describe.
 * @param describe Salesforce's report describe.
 * @returns The definition.
 */
export function reportDescriptionFrom(describe: Record<string, unknown>): ReportDescription {
    const metadata = objectAt(describe, "reportMetadata");
    const extended = objectAt(describe, "reportExtendedMetadata");
    const groupings = groupingColumns(arrayAt(metadata, "groupingsDown"), objectAt(extended, "groupingColumnInfo"));
    for (const column of groupingColumns(arrayAt(metadata, "groupingsAcross"), objectAt(extended, "groupingColumnInfo"))) groupings.push(column);
    const filters: SavedFilter[] = [];
    for (const filter of arrayAt(metadata, "reportFilters")) {
        filters.push({ column: textAt(filter, "column") ?? "", operator: textAt(filter, "operator") ?? "", value: textAt(filter, "value") ?? "" });
    }
    const logic = textAt(metadata, "reportBooleanFilter");
    return {
        id: textAt(metadata, "id") ?? "",
        name: textAt(metadata, "name") ?? "",
        format: textAt(metadata, "reportFormat") ?? "",
        reportType: textAt(objectAt(metadata, "reportType"), "type") ?? "",
        columns: columnsOf(arrayAt(metadata, "detailColumns"), objectAt(extended, "detailColumnInfo")),
        groupings: groupings,
        aggregates: columnsOf(arrayAt(metadata, "aggregates"), objectAt(extended, "aggregateColumnInfo")),
        filters: filters,
        booleanFilter: logic !== null && logic.trim().length > 0 ? logic : null,
    };
}

/**
 * A dashboard and its components' data.
 * @param body Salesforce's dashboard result.
 * @returns The dashboard.
 */
export function dashboardFrom(body: Record<string, unknown>): Dashboard {
    const metadata = objectAt(body, "dashboardMetadata");
    const data: Record<string, unknown>[] = [];
    for (const item of arrayAt(body, "componentData")) data.push(item as Record<string, unknown>);
    const components: DashboardComponent[] = [];
    for (const component of arrayAt(metadata, "components")) {
        const id = textAt(component, "id") ?? "";
        let found: Record<string, unknown> = {};
        for (const item of data) {
            if (textAt(item, "componentId") === id) found = item;
        }
        const result = objectAt(found, "reportResult");
        const flat = Object.keys(result).length > 0 ? reportFrom(result, false) : null;
        const properties = objectAt(component, "properties");
        components.push({
            id: id,
            header: textAt(component, "header"),
            title: textAt(component, "title"),
            reportId: textAt(component, "reportId"),
            type: textAt(properties, "visualizationType") ?? textAt(component, "type"),
            refreshDate: textAt(objectAt(found, "status"), "refreshDate"),
            groupings: flat === null ? [] : flat.groupings,
            aggregates: flat === null ? [] : flat.aggregates,
            totals: flat === null ? [] : flat.totals,
        });
    }
    return {
        id: textAt(metadata, "id") ?? "",
        name: textAt(metadata, "name") ?? "",
        runningUserId: textAt(objectAt(metadata, "runningUser"), "id"),
        components: components,
    };
}

interface GroupNode {
    key: string;
    groups: Record<string, FieldValue>;
    leaf: boolean;
}

// Every node of a grouping tree with its path's values, and the total "T", which
// is the only leaf when there are no groupings.
function groupNodes(tree: Record<string, unknown>, columns: ReportColumn[]): GroupNode[] {
    const nodes: GroupNode[] = [];
    walk(arrayAt(tree, "groupings"), columns, 0, {}, nodes);
    nodes.push({ key: "T", groups: {}, leaf: columns.length === 0 });
    return nodes;
}

function walk(groupings: unknown[], columns: ReportColumn[], depth: number, path: Record<string, FieldValue>, out: GroupNode[]): void {
    if (depth >= columns.length) return;
    const column = columns[depth];
    for (const grouping of groupings) {
        const groups: Record<string, FieldValue> = {};
        for (const key of Object.keys(path)) groups[key] = path[key];
        groups[column.name] = cellValue(grouping, column.dataType);
        out.push({ key: textAt(grouping, "key") ?? "", groups: groups, leaf: depth === columns.length - 1 });
        walk(arrayAt(grouping, "groupings"), columns, depth + 1, groups, out);
    }
}

function columnsOf(names: unknown[], info: Record<string, unknown>): ReportColumn[] {
    const out: ReportColumn[] = [];
    for (const name of names) {
        if (typeof name !== "string") continue;
        const column = objectAt(info, name);
        out.push({ name: name, label: textAt(column, "label") ?? name, dataType: textAt(column, "dataType") ?? "" });
    }
    return out;
}

function groupingColumns(groupings: unknown[], info: Record<string, unknown>): ReportColumn[] {
    const names: unknown[] = [];
    for (const grouping of groupings) names.push(textAt(grouping, "name"));
    return columnsOf(names, info);
}

// A cell or group value: a number, date or boolean as its value, anything else as
// its displayed label (a lookup's name rather than its id).
function cellValue(cell: unknown, dataType: string): FieldValue {
    if (cell === null || cell === undefined || typeof cell !== "object") return null;
    const value = (cell as Record<string, unknown>)["value"];
    if (value === null || value === undefined) return null;
    if (typeof value === "object" && !Array.isArray(value)) {
        const amount = numberAt(value, "amount");
        if (amount !== null) return amount;
    }
    const type = dataType.toLowerCase();
    if (NUMBER_TYPES.includes(type) || VALUE_TYPES.includes(type)) {
        if (typeof value === "number" || typeof value === "boolean" || typeof value === "string") return value;
    }
    const label = textAt(cell, "label");
    if (label !== null) return label;
    return typeof value === "string" || typeof value === "number" || typeof value === "boolean" ? value : null;
}

function aggregateValue(cell: unknown): FieldValue {
    const value = numberAt(cell, "value");
    if (value !== null) return value;
    return textAt(cell, "value");
}
