// Request side of the Wix client: the input types, input normalization and
// validation, and the JSON builders. Internal: `src/lib.ts` re-exports the input
// types, and programs reach the helpers only through its operations.

import { validationError } from "./errors";

const DEFAULT_PAGE_SIZE = 50;
const MAX_PAGE_SIZE = 100;
const MAX_NOTE_LENGTH = 5000;
const MAX_MESSAGE_LENGTH = 1000;
const MAX_REASON_LENGTH = 200;
const MAX_TRACKING_LENGTH = 100;
const MAX_LINE_ITEMS = 300;
const MAX_QUANTITY = 100000;
const MAX_SORTS = 4;
const MAX_SEARCH_LENGTH = 100;
const ORDER_SORT_FIELDS = ["createdDate", "updatedDate", "number", "status", "paymentStatus", "fulfillmentStatus", "priceSummary.total.amount"];
const PLAIN_DIGITS = "0123456789";

/** The Wix Stores app, which owns every Stores catalog item in a cart. */
export const STORES_APP_ID = "215238eb-22a5-4c36-9e7b-e7c08025e04e";

/** Cursor pagination for list calls. */
export interface PageOptions {
    /** Page size, 1-100 (default 50). */
    limit?: number;
    /** Cursor from a previous page's `nextCursor`. A cursor carries its filter and sort, so later pages repeat neither. */
    cursor?: string;
}

/** One sort for `searchOrders`. */
export interface OrderSort {
    /** One of `createdDate`, `updatedDate`, `number`, `status`, `paymentStatus`, `fulfillmentStatus`, `priceSummary.total.amount`. */
    field: string;
    /** `ASC` or `DESC`. */
    order: string;
}

/** Filters for `searchOrders`, combined with AND. */
export interface OrderQuery {
    /** `PENDING`, `APPROVED`, `CANCELED` or `REJECTED`. Wix leaves `PENDING` and `REJECTED` orders out unless they are asked for here. */
    status?: string;
    /** Such as `PAID`, `NOT_PAID`, `PARTIALLY_REFUNDED`, `FULLY_REFUNDED`. */
    paymentStatus?: string;
    /** `NOT_FULFILLED`, `PARTIALLY_FULFILLED` or `FULFILLED`. */
    fulfillmentStatus?: string;
    /** Buyer's email; trimmed and lowercased. */
    buyerEmail?: string;
    /** Orders created at or after this ISO-8601 instant, such as `2026-10-01T00:00:00Z`. */
    createdAfter?: string;
    /** The order number shown in the dashboard. */
    number?: number;
    /** Up to four sorts. Wix sorts by `createdDate` descending by default. */
    sort?: OrderSort[];
}

/** Filters for `listAbandonedCheckouts`, combined with AND. */
export interface AbandonedCheckoutFilter {
    /** `ABANDONED` or `RECOVERED`. */
    status?: string;
    /** Buyer's email; trimmed and lowercased. */
    buyerEmail?: string;
    /** Checkouts created at or after this ISO-8601 instant. */
    createdAfter?: string;
}

/** Choices for `cancelOrder`. */
export interface CancelInput {
    /** Put every line item back in Wix Stores inventory. Default false. */
    restock?: boolean;
    /** Send the buyer the order-canceled email. Default false. */
    notifyCustomer?: boolean;
    /** Note for that email, up to 1000 characters; needs `notifyCustomer`. */
    message?: string;
}

/** One order line and a quantity. */
export interface LineItemQuantity {
    /** Line item id, from `order.lineItems`. */
    lineItemId: string;
    /** Quantity, 1-100000. Optional for a fulfillment, where it defaults to what remains unfulfilled. */
    quantity?: number;
}

/** Where a shipment can be tracked. */
export interface TrackingInput {
    /** Tracking number, up to 100 characters. */
    number: string;
    /** Carrier, such as `fedex`, `ups` or `usps`, or any other carrier's name. */
    provider: string;
    /** Tracking page, `https://` only. Wix fills it in for the carriers it knows. */
    link?: string;
}

/** A fulfillment to record. */
export interface FulfillmentInput {
    /** Where the shipment can be tracked. Wix requires it. */
    tracking: TrackingInput;
    /** Lines to fulfill. Omit it to fulfill every line of the order. */
    lineItems?: LineItemQuantity[];
}

/** New tracking details. Unset fields stay as they are. */
export interface TrackingUpdate {
    /** Tracking number, up to 100 characters. */
    number?: string;
    /** Carrier name. */
    provider?: string;
    /** Tracking page, `https://` only. */
    link?: string;
}

/** A refund of one payment. */
export interface RefundInput {
    /** The payment to refund, from `listPayments`. */
    paymentId: string;
    /** Amount in the order's currency, above 0, at most three decimal places. */
    amount: number;
    /** Lines being refunded, with quantities. Needed for `restock`. */
    lineItems?: LineItemQuantity[];
    /** Put `lineItems` back in inventory. Default false. */
    restock?: boolean;
    /** Send the buyer a refund email. Default false. */
    notifyCustomer?: boolean;
    /** Message for that email; needs `notifyCustomer`. */
    message?: string;
    /** Reason recorded on the refund, up to 200 characters. */
    reason?: string;
    /** Record a refund made outside Wix, at the payment provider, without calling the provider. Default false. */
    externalRefund?: boolean;
}

/** One catalog item for a cart. */
export interface CartItemInput {
    /** Stores product id. */
    productId: string;
    /** Quantity, 1-100000. */
    quantity: number;
    /** Variant id, from `getInventory`, for a product with variants. */
    variantId?: string;
}

/** Who a cart is for. */
export interface CartCustomer {
    /** Customer's email; trimmed and lowercased. */
    email?: string;
    /** Customer's first name. */
    firstName?: string;
    /** Customer's last name. */
    lastName?: string;
}

/** A catalog search. */
export interface ProductQuery {
    /** Search text, up to 100 characters. Omit it to list products. */
    text?: string;
    /** Include products hidden from the storefront. Default false. */
    includeHidden?: boolean;
}

/** Product fields to change. Unset fields stay as they are. */
export interface ProductUpdate {
    /** Product name, 1-80 characters. */
    name?: string;
    /** Product description, as HTML. */
    description?: string;
    /** Whether site visitors see the product. */
    visible?: boolean;
    /** Ribbon text, such as `Sale`. Catalog V1 only. */
    ribbon?: string;
    /** Price in the store's currency. Catalog V1 only. */
    price?: number;
}

/** One validated line and quantity. */
export interface PreparedLine {
    lineItemId: string;
    quantity: number | null;
}

/** One validated cart item. */
export interface PreparedCartItem {
    productId: string;
    quantity: number;
    variantId: string | null;
}

/** One validated page request. */
export interface PageWindow {
    limit: number;
    cursor: string | null;
}

export function isGuid(value: string): boolean {
    if (value.length !== 36) return false;
    for (let i = 0; i < 36; i = i + 1) {
        const c = value.charAt(i);
        if (i === 8 || i === 13 || i === 18 || i === 23) {
            if (c !== "-") return false;
        } else if (!"0123456789abcdef".includes(c)) {
            return false;
        }
    }
    return true;
}

// Trim and lowercase an id and require the GUID form, so a filter on it sees one spelling.
export function normalizeId(value: string, name: string): string {
    const id = value.trim().toLowerCase();
    if (!isGuid(id)) {
        throw validationError("invalid_id", name + " must be a Wix id (a GUID), got " + JSON.stringify(value));
    }
    return id;
}

export function normalizeSiteId(value: string): string {
    const id = value.trim().toLowerCase();
    if (!isGuid(id)) throw validationError("invalid_site_id", "WIX_SITE_ID must be a site id (a GUID)");
    return id;
}

export function normalizeEmail(value: string): string {
    const email = value.trim().toLowerCase();
    const at = email.indexOf("@");
    if (at < 1 || at !== email.lastIndexOf("@") || at === email.length - 1 || email.includes(" ")) {
        throw validationError("invalid_email", "not an email address: " + JSON.stringify(value));
    }
    return email;
}

// An amount of money: finite, above zero, at most three decimal places, so the request cannot round it.
export function normalizeAmount(value: number, name: string): number {
    if (!Number.isFinite(value) || value <= 0 || value > 1000000000) {
        throw validationError("invalid_amount", name + " must be above 0, got " + value.toString());
    }
    if (Math.round(value * 1000) / 1000 !== value) {
        throw validationError("invalid_amount", name + " has more than three decimal places: " + value.toString());
    }
    return value;
}

export function normalizeQuantity(value: number, name: string): number {
    if (!Number.isInteger(value) || value < 1 || value > MAX_QUANTITY) {
        throw validationError("invalid_quantity", name + " must be a whole number from 1 to 100000, got " + value.toString());
    }
    return value;
}

// A non-zero whole change in stock.
export function normalizeDelta(value: number): number {
    if (!Number.isInteger(value) || value === 0 || Math.abs(value) > MAX_QUANTITY) {
        throw validationError("invalid_delta", "delta must be a non-zero whole number up to 100000 either way, got " + value.toString());
    }
    return value;
}

export function requireText(value: string, name: string, maxLength: number): string {
    const text = value.trim();
    if (text.length === 0 || text.length > maxLength) {
        throw validationError("invalid_text", name + " must be 1 to " + maxLength.toString() + " characters");
    }
    return text;
}

export function normalizeNote(value: string): string {
    return requireText(value, "message", MAX_NOTE_LENGTH);
}

// Validate an email message, which is sent only with a notification.
export function optionalMessage(value: string | undefined, notify: boolean): string | null {
    if (value === undefined) return null;
    if (!notify) throw validationError("message_without_notification", "message is sent by email; set notifyCustomer");
    return requireText(value, "message", MAX_MESSAGE_LENGTH);
}

export function optionalReason(value: string | undefined): string | null {
    if (value === undefined) return null;
    return requireText(value, "reason", MAX_REASON_LENGTH);
}

export function normalizeProvider(value: string): string {
    return requireText(value, "tracking provider", MAX_TRACKING_LENGTH);
}

export function normalizeTrackingNumber(value: string): string {
    return requireText(value, "tracking number", MAX_TRACKING_LENGTH);
}

export function normalizeTrackingLink(value: string): string {
    const link = value.trim();
    if (!link.startsWith("https://") || link.length > 2048 || link.includes(" ")) {
        throw validationError("invalid_tracking_link", "tracking link must be an https:// URL");
    }
    return link;
}

// An ISO-8601 instant, parsed by the runtime's Temporal so Wix never sees a malformed date.
export function normalizeInstant(value: string, name: string): string {
    const text = value.trim();
    try {
        return Temporal.Instant.from(text).toString();
    } catch (error) {
        throw validationError("invalid_date", name + " must be an ISO-8601 instant such as 2026-10-01T00:00:00Z, got " + JSON.stringify(value));
    }
}

export function pageWindow(page: PageOptions): PageWindow {
    const { limit, cursor } = page;
    const size = limit ?? DEFAULT_PAGE_SIZE;
    if (!Number.isInteger(size) || size < 1 || size > MAX_PAGE_SIZE) {
        throw validationError("invalid_limit", "limit must be between 1 and 100, got " + size.toString());
    }
    if (cursor !== undefined && cursor.length === 0) {
        throw validationError("invalid_cursor", "cursor must not be empty");
    }
    return { limit: size, cursor: cursor ?? null };
}

// Validate one line. The exported operation copies the caller's array with one `for...of`.
export function prepareLine(lineItemId: string, quantity: number | undefined): PreparedLine {
    return {
        lineItemId: normalizeId(lineItemId, "lineItemId"),
        quantity: quantity === undefined ? null : normalizeQuantity(quantity, "quantity"),
    };
}

export function prepareCartItem(productId: string, quantity: number, variantId: string | undefined): PreparedCartItem {
    return {
        productId: normalizeId(productId, "productId"),
        quantity: normalizeQuantity(quantity, "quantity"),
        variantId: variantId === undefined ? null : normalizeId(variantId, "variantId"),
    };
}

export function requireLineCount(count: number): void {
    if (count === 0 || count > MAX_LINE_ITEMS) throw validationError("invalid_line_items", "give 1 to 300 lines");
}

// Sorted names of the fields an update sets, the list a policy filters on.
export function productFields(name: boolean, description: boolean, visible: boolean, ribbon: boolean, price: boolean): string[] {
    const fields: string[] = [];
    if (description) fields.push("description");
    if (name) fields.push("name");
    if (price) fields.push("price");
    if (ribbon) fields.push("ribbon");
    if (visible) fields.push("visible");
    if (fields.length === 0) throw validationError("empty_update", "name at least one field to change");
    return fields;
}

function searchEnvelope(filter: Record<string, unknown>, sort: Record<string, unknown>[], window: PageWindow): Record<string, unknown> {
    const paging: Record<string, unknown> = { limit: window.limit };
    if (window.cursor !== null) {
        // A Wix cursor carries the filter and the sort; sending them again is an error.
        paging["cursor"] = window.cursor;
        return { cursorPaging: paging };
    }
    const envelope: Record<string, unknown> = { cursorPaging: paging };
    if (Object.keys(filter).length > 0) envelope["filter"] = filter;
    if (sort.length > 0) envelope["sort"] = sort;
    return envelope;
}

export function buildOrderSearch(query: OrderQuery, window: PageWindow): Record<string, unknown> {
    const { status, paymentStatus, fulfillmentStatus, buyerEmail, createdAfter, number, sort } = query;
    const filter: Record<string, unknown> = {};
    if (status !== undefined) filter["status"] = requireText(status, "status", 40).toUpperCase();
    if (paymentStatus !== undefined) filter["paymentStatus"] = requireText(paymentStatus, "paymentStatus", 40).toUpperCase();
    if (fulfillmentStatus !== undefined) filter["fulfillmentStatus"] = requireText(fulfillmentStatus, "fulfillmentStatus", 40).toUpperCase();
    if (buyerEmail !== undefined) filter["buyerInfo.email"] = normalizeEmail(buyerEmail);
    if (createdAfter !== undefined) {
        const after: Record<string, unknown> = {};
        after["$gte"] = normalizeInstant(createdAfter, "createdAfter");
        filter["createdDate"] = after;
    }
    if (number !== undefined) {
        if (!Number.isSafeInteger(number) || number < 0) {
            throw validationError("invalid_number", "number must be a whole order number, got " + number.toString());
        }
        filter["number"] = number;
    }
    const sorts: Record<string, unknown>[] = [];
    for (const item of sort ?? []) {
        const { field, order } = item;
        if (!ORDER_SORT_FIELDS.includes(field)) {
            throw validationError("invalid_sort", "cannot sort orders by " + JSON.stringify(field));
        }
        if (order !== "ASC" && order !== "DESC") {
            throw validationError("invalid_sort", "sort order must be ASC or DESC, got " + JSON.stringify(order));
        }
        sorts.push({ fieldName: field, order: order });
    }
    if (sorts.length > MAX_SORTS) throw validationError("invalid_sort", "at most four sorts");
    return { search: searchEnvelope(filter, sorts, window) };
}

export function buildAbandonedQuery(query: AbandonedCheckoutFilter, window: PageWindow): Record<string, unknown> {
    const { status, buyerEmail, createdAfter } = query;
    const filter: Record<string, unknown> = {};
    if (status !== undefined) filter["status"] = requireText(status, "status", 40).toUpperCase();
    if (buyerEmail !== undefined) filter["buyerInfo.email"] = normalizeEmail(buyerEmail);
    if (createdAfter !== undefined) {
        const after: Record<string, unknown> = {};
        after["$gte"] = normalizeInstant(createdAfter, "createdAfter");
        filter["createdDate"] = after;
    }
    const newest: Record<string, unknown>[] = [{ fieldName: "createdDate", order: "DESC" }];
    return { query: searchEnvelope(filter, newest, window) };
}

export function buildCancelBody(restock: boolean, notify: boolean, message: string | null): Record<string, unknown> {
    const body: Record<string, unknown> = { restockAllItems: restock, sendOrderCanceledEmail: notify };
    if (message !== null) body["customMessage"] = message;
    return body;
}

export function buildNoteBody(message: string): Record<string, unknown> {
    return { orderActivities: [{ activityType: "MERCHANT_COMMENT", merchantComment: { message: message } }] };
}

export function buildTracking(number: string | null, provider: string | null, link: string | null): Record<string, unknown> {
    const tracking: Record<string, unknown> = {};
    if (number !== null) tracking["trackingNumber"] = number;
    if (provider !== null) tracking["shippingProvider"] = provider;
    if (link !== null) tracking["trackingLink"] = link;
    return tracking;
}

export function buildFulfillmentBody(lines: PreparedLine[], tracking: Record<string, unknown>): Record<string, unknown> {
    const lineItems: Record<string, unknown>[] = [];
    for (const line of lines) {
        const item: Record<string, unknown> = { id: line.lineItemId };
        if (line.quantity !== null) item["quantity"] = line.quantity;
        lineItems.push(item);
    }
    return { fulfillment: { lineItems: lineItems, trackingInfo: tracking } };
}

// A decimal string for Wix, which takes amounts as text.
export function decimalText(amount: number): string {
    return amount.toString();
}

function refundLines(lines: PreparedLine[]): Record<string, unknown>[] {
    const items: Record<string, unknown>[] = [];
    for (const line of lines) {
        const item: Record<string, unknown> = { lineItemId: line.lineItemId };
        item["quantity"] = line.quantity ?? 1;
        items.push(item);
    }
    return items;
}

export function buildRefundBody(
    orderId: string,
    paymentId: string,
    amount: number,
    lines: PreparedLine[],
    restock: boolean,
    externalRefund: boolean,
    notify: boolean,
    message: string | null,
    reason: string | null,
): Record<string, unknown> {
    const payment: Record<string, unknown> = { paymentId: paymentId, amount: { amount: decimalText(amount) } };
    if (externalRefund) payment["externalRefund"] = true;
    const body: Record<string, unknown> = { orderId: orderId, paymentRefunds: [payment] };
    const sideEffects: Record<string, unknown> = {};
    if (restock) sideEffects["restock"] = { lineItems: refundLines(lines) };
    if (notify) {
        const notifications: Record<string, unknown> = { sendCustomerEmail: true };
        if (message !== null) notifications["customMessage"] = message;
        sideEffects["notifications"] = notifications;
    }
    if (Object.keys(sideEffects).length > 0) body["sideEffects"] = sideEffects;
    if (lines.length > 0) body["refundItems"] = { lineItems: refundLines(lines) };
    if (reason !== null) body["customerReason"] = reason;
    return body;
}

export function buildCartBody(items: PreparedCartItem[], email: string | null, firstName: string | null, lastName: string | null): Record<string, unknown> {
    const catalogItems: Record<string, unknown>[] = [];
    for (const item of items) {
        const reference: Record<string, unknown> = { catalogItemId: item.productId, appId: STORES_APP_ID };
        if (item.variantId !== null) reference["options"] = { variantId: item.variantId };
        catalogItems.push({ catalogReference: reference, quantity: item.quantity });
    }
    const body: Record<string, unknown> = { catalogItems: catalogItems };
    const customer: Record<string, unknown> = {};
    if (email !== null) customer["email"] = email;
    if (firstName !== null) customer["firstName"] = firstName;
    if (lastName !== null) customer["lastName"] = lastName;
    if (Object.keys(customer).length > 0) body["cart"] = { customerInfo: customer };
    return body;
}

// A catalog V1 filter is JSON inside a string.
export function buildProductQueryV1(text: string | null, includeHidden: boolean, limit: number, offset: number): Record<string, unknown> {
    const query: Record<string, unknown> = { paging: { limit: limit, offset: offset } };
    if (text !== null) {
        const contains: Record<string, unknown> = {};
        contains["$contains"] = text;
        const filter: Record<string, unknown> = {};
        filter["name"] = contains;
        query["filter"] = JSON.stringify(filter);
    }
    return { query: query, includeHiddenProducts: includeHidden };
}

export function buildProductSearchV3(text: string | null, includeHidden: boolean, window: PageWindow): Record<string, unknown> {
    const filter: Record<string, unknown> = {};
    if (!includeHidden) filter["visible"] = true;
    const search = searchEnvelope(filter, [], window);
    if (text !== null && window.cursor === null) search["search"] = { expression: text };
    return { search: search, fields: ["CURRENCY", "PLAIN_DESCRIPTION"] };
}

export function requireSearchText(text: string | undefined): string | null {
    if (text === undefined) return null;
    return requireText(text, "text", MAX_SEARCH_LENGTH);
}

// Read a catalog V1 page cursor, which is an offset in digits.
export function offsetFrom(cursor: string | null): number {
    if (cursor === null) return 0;
    for (const c of cursor.split("")) {
        if (!PLAIN_DIGITS.includes(c)) throw validationError("invalid_cursor", "not a cursor from this catalog: " + JSON.stringify(cursor));
    }
    return Number(cursor);
}

export function buildProductPatchV1(productId: string, name: string | null, description: string | null, visible: boolean | null, ribbon: string | null, price: number | null): Record<string, unknown> {
    const product: Record<string, unknown> = { id: productId };
    if (name !== null) product["name"] = name;
    if (description !== null) product["description"] = description;
    if (visible !== null) product["visible"] = visible;
    if (ribbon !== null) product["ribbon"] = ribbon;
    if (price !== null) product["priceData"] = { price: price };
    return { product: product };
}

export function buildProductPatchV3(productId: string, revision: string, name: string | null, description: string | null, visible: boolean | null): Record<string, unknown> {
    const product: Record<string, unknown> = { id: productId, revision: revision };
    if (name !== null) product["name"] = name;
    if (description !== null) product["plainDescription"] = description;
    if (visible !== null) product["visible"] = visible;
    return { product: product };
}

// A catalog V1 filter is JSON inside a string.
export function buildInventoryQueryV1(productId: string): Record<string, unknown> {
    const filter: Record<string, unknown> = { productId: productId };
    return { query: { filter: JSON.stringify(filter) } };
}

export function buildInventoryQueryV3(productId: string): Record<string, unknown> {
    return { query: { filter: { productId: productId }, cursorPaging: { limit: MAX_PAGE_SIZE } } };
}

export function buildAdjustV1(productId: string, variantId: string, delta: number): Record<string, unknown> {
    if (delta > 0) return { incrementData: [{ productId: productId, variantId: variantId, incrementBy: delta }] };
    return { decrementData: [{ productId: productId, variantId: variantId, decrementBy: -delta }] };
}

export function buildAdjustV3(inventoryItemId: string, delta: number): Record<string, unknown> {
    if (delta > 0) return { incrementData: [{ inventoryItemId: inventoryItemId, incrementBy: delta }], reason: "MANUAL" };
    return { decrementData: [{ inventoryItemId: inventoryItemId, decrementBy: -delta }], reason: "MANUAL" };
}

export function buildContactQuery(email: string): Record<string, unknown> {
    const filter: Record<string, unknown> = {};
    filter["info.emails.email"] = email;
    return { query: { filter: filter, paging: { limit: 2 }, fieldsets: ["FULL"] } };
}
