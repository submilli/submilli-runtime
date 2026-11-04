// Request side of the Shopify client: the input types, input normalization and
// validation, and the variable builders. Internal: `src/lib.ts` re-exports the
// input types, and programs reach the helpers only through its operations.

import { validationError } from "./errors";

const DEFAULT_PAGE_SIZE = 50;
const MAX_PAGE_SIZE = 100;
const MAX_INVENTORY_PAGE_SIZE = 15;
const MAX_QUERY_LENGTH = 1000;
const MAX_NOTE_LENGTH = 5000;
const MAX_STAFF_NOTE_LENGTH = 255;
const MAX_TRACKING_LENGTH = 255;
const MAX_TAG_LENGTH = 40;
const MAX_TAGS = 250;
const MAX_LINES = 100;
const MAX_QUANTITY = 1000000;
const MAX_KEY_LENGTH = 255;
const ORDER_SORT_KEYS = ["CREATED_AT", "UPDATED_AT", "PROCESSED_AT", "ORDER_NUMBER", "CURRENT_TOTAL_PRICE"];
const CANCEL_REASONS = ["CUSTOMER", "DECLINED", "FRAUD", "INVENTORY", "STAFF", "OTHER"];
const HOLD_REASONS = ["AWAITING_PAYMENT", "HIGH_RISK_OF_FRAUD", "INCORRECT_ADDRESS", "INVENTORY_OUT_OF_STOCK", "UNKNOWN_DELIVERY_DATE", "AWAITING_RETURN_ITEMS", "OTHER"];
const PRODUCT_STATUSES = ["ACTIVE", "ARCHIVED", "DRAFT", "UNLISTED"];
const SHOP_CHARACTERS = "abcdefghijklmnopqrstuvwxyz0123456789-";
const DIGITS = "0123456789";

/** `first` and `after` for a list call. */
export interface PageOptions {
    /** Page size, 1-100 (default 50). */
    first?: number;
    /** Cursor from a previous page's `nextCursor`. */
    after?: string;
}

/** A search over one kind of record. */
export interface SearchInput {
    /** Shopify search syntax, such as `status:active vendor:Acme`. Omit it to list. */
    query?: string;
    /** Page size, 1-100 (default 50). */
    first?: number;
    /** Cursor from a previous page's `nextCursor`. Repeat the same `query`. */
    after?: string;
}

/** A search over orders. */
export interface OrderSearchInput {
    /** Shopify search syntax, such as `name:#1001 financial_status:paid created_at:>'2026-10-01'`. */
    query?: string;
    /** Page size, 1-100 (default 50). */
    first?: number;
    /** Cursor from a previous page's `nextCursor`. Repeat the same `query` and sort. */
    after?: string;
    /** `CREATED_AT` (default), `UPDATED_AT`, `PROCESSED_AT`, `ORDER_NUMBER` or `CURRENT_TOTAL_PRICE`. */
    sortKey?: string;
    /** Newest or largest first. Default true. */
    reverse?: boolean;
}

/** Choices for `cancelOrder`. */
export interface CancelInput {
    /** `CUSTOMER`, `DECLINED`, `FRAUD`, `INVENTORY`, `STAFF` or `OTHER`. */
    reason: string;
    /** Return the items to inventory. */
    restock: boolean;
    /** Refund to the original payment method. */
    refund: boolean;
    /** Tell the customer. Default false. */
    notifyCustomer?: boolean;
    /** Note for staff, up to 255 characters. */
    staffNote?: string;
}

/** One line of an order and a quantity. */
export interface RefundLineInput {
    /** Line item id, from `order.lineItems`. */
    lineItemId: string;
    /** Quantity to refund. */
    quantity: number;
}

/** What to price for a refund. */
export interface RefundQuery {
    /** Lines to refund. */
    lineItems?: RefundLineInput[];
    /** Refund the remaining shipping in full. Default false. */
    shipping?: boolean;
}

/** A refund. Shopify works out the amounts and the payment transactions. */
export interface RefundInput {
    /** Lines to refund. */
    lineItems?: RefundLineInput[];
    /** Refund the remaining shipping in full. Default false. */
    shipping?: boolean;
    /** `NO_RESTOCK` (default), `RETURN` for fulfilled items, or `CANCEL` for unfulfilled items. */
    restockType?: string;
    /** Location to restock at; required unless `restockType` is `NO_RESTOCK`. */
    locationId?: string;
    /** Note on the refund for staff. */
    note?: string;
    /** Tell the customer. Default false. */
    notify?: boolean;
    /** A new unique string for each refund, such as a UUID. Repeating a call with the same key refunds once. */
    idempotencyKey: string;
}

/** Where a shipment can be tracked. */
export interface TrackingInput {
    /** Carrier name. */
    company?: string;
    /** Tracking number. */
    number?: string;
    /** Tracking page, `https://` only. */
    url?: string;
}

/** A fulfillment to record. */
export interface FulfillmentInput {
    /** Fulfillment orders to fulfill, all at one location. Omit it for every open fulfillment order of the order. */
    fulfillmentOrderIds?: string[];
    /** Where the shipment can be tracked. */
    tracking?: TrackingInput;
    /** Tell the customer. Default false. */
    notifyCustomer?: boolean;
}

/** Product fields to change. Unset fields stay as they are. */
export interface ProductUpdate {
    /** Title. */
    title?: string;
    /** Description, as HTML. */
    descriptionHtml?: string;
    /** `ACTIVE`, `ARCHIVED`, `DRAFT` or `UNLISTED`. */
    status?: string;
    /** Tags; replaces the product's tags. */
    tags?: string[];
    /** Vendor. */
    vendor?: string;
    /** Product type. */
    productType?: string;
}

/** A new price for one variant. */
export interface VariantPriceInput {
    /** Variant id. */
    variantId: string;
    /** New price, above 0. */
    price: number;
    /** Price to show struck through, above 0. */
    compareAtPrice?: number;
}

/** A discount code that applies to the whole store. Set exactly one of `percentage` and `amount`. */
export interface DiscountInput {
    /** Name staff see. */
    title: string;
    /** Code the buyer enters; uppercased. */
    code: string;
    /** Percent off, above 0 and at most 100. */
    percentage?: number;
    /** Fixed amount off the order. */
    amount?: number;
    /** When it starts, an ISO-8601 instant. */
    startsAt: string;
    /** When it ends, an ISO-8601 instant. */
    endsAt?: string;
    /** How many times it can be used in total. Omit it for no limit. */
    usageLimit?: number;
    /** Limit each customer to one use. Default false. */
    oncePerCustomer?: boolean;
    /** Smallest order subtotal it applies to. */
    minimumSubtotal?: number;
}

/** One variant and a quantity for a draft order. */
export interface DraftLineInput {
    /** Variant id. */
    variantId: string;
    /** Quantity. */
    quantity: number;
}

/** A discount across a draft order. */
export interface DraftDiscountInput {
    /** Percent off, above 0 and at most 100. */
    percentage: number;
    /** Label the buyer sees. */
    title?: string;
}

/** A draft order. */
export interface DraftOrderInput {
    /** Variants and quantities, 1-100 lines. */
    lineItems: DraftLineInput[];
    /** Buyer's email; trimmed and lowercased. */
    email?: string;
    /** Note for staff. */
    note?: string;
    /** A discount across the order. */
    discount?: DraftDiscountInput;
}

/** One validated refund line. */
export interface PreparedRefundLine {
    lineItemId: string;
    quantity: number;
}

/** One validated draft order line. */
export interface PreparedDraftLine {
    variantId: string;
    quantity: number;
}

/** One validated variant price. */
export interface PreparedVariantPrice {
    variantId: string;
    price: number;
    compareAtPrice: number | null;
}

/** One validated page request. */
export interface PageWindow {
    first: number;
    after: string | null;
}

// The store's subdomain: a single DNS label, so the host is always `<label>.myshopify.com`.
export function normalizeShop(value: string): string {
    const shop = value.trim().toLowerCase();
    let valid = shop.length >= 1 && shop.length <= 63 && !shop.startsWith("-") && !shop.endsWith("-");
    for (const c of shop.split("")) {
        if (!SHOP_CHARACTERS.includes(c)) valid = false;
    }
    if (!valid) throw validationError("invalid_shop", "SHOPIFY_SHOP must be the store's myshopify subdomain, a single label such as acme-store");
    return shop;
}

function allDigits(value: string): boolean {
    // A leading zero would be a second spelling of the same id.
    if (value.length === 0 || value.length > 20 || value.startsWith("0")) return false;
    for (const c of value.split("")) {
        if (!DIGITS.includes(c)) return false;
    }
    return true;
}

// A number or a Shopify global id, in one spelling, so a filter sees one form.
export function normalizeGid(value: string, type: string, name: string): string {
    const text = value.trim();
    const prefix = "gid://shopify/" + type + "/";
    const digits = text.startsWith(prefix) ? text.slice(prefix.length) : text;
    if (!allDigits(digits)) {
        throw validationError("invalid_id", name + " must be a numeric id or a " + prefix + " id, got " + JSON.stringify(value));
    }
    return prefix + digits;
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

export function normalizePercentage(value: number, name: string): number {
    if (!Number.isFinite(value) || value <= 0 || value > 100) {
        throw validationError("invalid_percentage", name + " must be above 0 and at most 100, got " + value.toString());
    }
    return value;
}

export function normalizeQuantity(value: number, name: string): number {
    if (!Number.isInteger(value) || value < 1 || value > MAX_QUANTITY) {
        throw validationError("invalid_quantity", name + " must be a whole number from 1 to 1000000, got " + value.toString());
    }
    return value;
}

// A stock level to set: zero is allowed, unlike a quantity to move.
export function normalizeStockLevel(value: number): number {
    if (!Number.isInteger(value) || value < 0 || value > MAX_QUANTITY) {
        throw validationError("invalid_quantity", "quantity must be a whole number from 0 to 1000000, got " + value.toString());
    }
    return value;
}

export function normalizeDelta(value: number): number {
    if (!Number.isInteger(value) || value === 0 || Math.abs(value) > MAX_QUANTITY) {
        throw validationError("invalid_delta", "delta must be a non-zero whole number up to 1000000 either way, got " + value.toString());
    }
    return value;
}

export function normalizeUsageLimit(value: number): number {
    if (!Number.isInteger(value) || value < 1 || value > MAX_QUANTITY) {
        throw validationError("invalid_usage_limit", "usageLimit must be a whole number from 1 to 1000000, got " + value.toString());
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
    return requireText(value, "note", MAX_NOTE_LENGTH);
}

export function normalizeStaffNote(value: string): string {
    return requireText(value, "staffNote", MAX_STAFF_NOTE_LENGTH);
}

export function normalizeKey(value: string): string {
    return requireText(value, "idempotencyKey", MAX_KEY_LENGTH);
}

export function normalizeTag(value: string): string {
    const tag = requireText(value, "tag", MAX_TAG_LENGTH);
    if (tag.includes(",")) throw validationError("invalid_tag", "a tag cannot hold a comma: " + JSON.stringify(value));
    return tag;
}

export function normalizeCode(value: string): string {
    return requireText(value, "code", 255).toUpperCase();
}

function oneOf(value: string, allowed: string[], name: string): string {
    const text = value.trim().toUpperCase();
    if (!allowed.includes(text)) throw validationError("invalid_" + name, name + " must be one of " + allowed.join(", ") + ", got " + JSON.stringify(value));
    return text;
}

export function normalizeCancelReason(value: string): string {
    return oneOf(value, CANCEL_REASONS, "reason");
}

export function normalizeHoldReason(value: string): string {
    return oneOf(value, HOLD_REASONS, "reason");
}

export function normalizeProductStatus(value: string): string {
    return oneOf(value, PRODUCT_STATUSES, "status");
}

export function normalizeRestockType(value: string | undefined): string {
    if (value === undefined) return "NO_RESTOCK";
    return oneOf(value, ["NO_RESTOCK", "RETURN", "CANCEL"], "restockType");
}

export function normalizeHttpsUrl(value: string): string {
    const link = value.trim();
    if (!link.startsWith("https://") || link.length > 2048 || link.includes(" ")) {
        throw validationError("invalid_url", "tracking url must be an https:// URL");
    }
    return link;
}

// An ISO-8601 instant, parsed by the runtime's Temporal so Shopify never sees a malformed date.
export function normalizeInstant(value: string, name: string): string {
    const text = value.trim();
    try {
        return Temporal.Instant.from(text).toString();
    } catch (error) {
        throw validationError("invalid_date", name + " must be an ISO-8601 instant such as 2026-10-01T00:00:00Z, got " + JSON.stringify(value));
    }
}

export function requireQuery(value: string | undefined): string | null {
    if (value === undefined) return null;
    return requireText(value, "query", MAX_QUERY_LENGTH);
}

function windowOf(first: number | undefined, after: string | undefined, maximum: number): PageWindow {
    const size = first ?? Math.min(DEFAULT_PAGE_SIZE, maximum);
    if (!Number.isInteger(size) || size < 1 || size > maximum) {
        throw validationError("invalid_limit", "first must be between 1 and " + maximum.toString() + ", got " + size.toString());
    }
    if (after !== undefined && after.length === 0) throw validationError("invalid_cursor", "after must not be empty");
    return { first: size, after: after ?? null };
}

export function pageWindow(first: number | undefined, after: string | undefined): PageWindow {
    return windowOf(first, after, MAX_PAGE_SIZE);
}

export function inventoryWindow(first: number | undefined, after: string | undefined): PageWindow {
    return windowOf(first, after, MAX_INVENTORY_PAGE_SIZE);
}

export function normalizeSortKey(value: string | undefined): string {
    if (value === undefined) return "CREATED_AT";
    return oneOf(value, ORDER_SORT_KEYS, "sortKey");
}

export function requireLineCount(count: number): void {
    if (count === 0 || count > MAX_LINES) throw validationError("invalid_line_items", "give 1 to 100 lines");
}

export function requireTagCount(count: number): void {
    if (count === 0 || count > MAX_TAGS) throw validationError("invalid_tags", "give 1 to 250 tags");
}

// Exactly one of a percentage and an amount, in the two fields a policy compares.
export function requireOneDiscountValue(percentage: number | null, amount: number | null): void {
    if ((percentage === null) === (amount === null)) {
        throw validationError("invalid_discount", "set exactly one of percentage and amount");
    }
}

export function prepareRefundLine(lineItemId: string, quantity: number): PreparedRefundLine {
    return { lineItemId: normalizeGid(lineItemId, "LineItem", "lineItemId"), quantity: normalizeQuantity(quantity, "quantity") };
}

export function prepareDraftLine(variantId: string, quantity: number): PreparedDraftLine {
    return { variantId: normalizeGid(variantId, "ProductVariant", "variantId"), quantity: normalizeQuantity(quantity, "quantity") };
}

export function prepareVariantPrice(variantId: string, price: number, compareAtPrice: number | undefined): PreparedVariantPrice {
    return {
        variantId: normalizeGid(variantId, "ProductVariant", "variantId"),
        price: normalizeAmount(price, "price"),
        compareAtPrice: compareAtPrice === undefined ? null : normalizeAmount(compareAtPrice, "compareAtPrice"),
    };
}

// Sorted names of the fields an update sets, the list a policy filters on.
export function productFields(title: boolean, descriptionHtml: boolean, status: boolean, tags: boolean, vendor: boolean, productType: boolean): string[] {
    const fields: string[] = [];
    if (descriptionHtml) fields.push("descriptionHtml");
    if (productType) fields.push("productType");
    if (status) fields.push("status");
    if (tags) fields.push("tags");
    if (title) fields.push("title");
    if (vendor) fields.push("vendor");
    if (fields.length === 0) throw validationError("empty_update", "name at least one field to change");
    return fields;
}

// A decimal string, which is how Shopify takes money.
export function moneyText(amount: number): string {
    return amount.toString();
}

function optional(variables: Record<string, unknown>, name: string, value: unknown): void {
    if (value !== null) variables[name] = value;
}

export function buildSearchVariables(window: PageWindow, query: string | null): Record<string, unknown> {
    const variables: Record<string, unknown> = { first: window.first };
    optional(variables, "after", window.after);
    optional(variables, "query", query);
    return variables;
}

export function buildOrderSearchVariables(window: PageWindow, query: string | null, sortKey: string, reverse: boolean): Record<string, unknown> {
    const variables = buildSearchVariables(window, query);
    variables["sortKey"] = sortKey;
    variables["reverse"] = reverse;
    return variables;
}

export function buildCancelVariables(orderId: string, reason: string, restock: boolean, refund: boolean, notify: boolean, staffNote: string | null): Record<string, unknown> {
    const variables: Record<string, unknown> = {
        orderId: orderId,
        reason: reason,
        restock: restock,
        refundMethod: { originalPaymentMethodsRefund: refund },
        notifyCustomer: notify,
    };
    optional(variables, "staffNote", staffNote);
    return variables;
}

// What the suggestion and the refund both send, so the amount checked is the amount refunded.
export function buildRefundLines(lines: PreparedRefundLine[], restockType: string, locationId: string | null): Record<string, unknown>[] {
    const items: Record<string, unknown>[] = [];
    for (const line of lines) {
        const item: Record<string, unknown> = { lineItemId: line.lineItemId, quantity: line.quantity, restockType: restockType };
        optional(item, "locationId", locationId);
        items.push(item);
    }
    return items;
}

export function buildTracking(company: string | null, number: string | null, url: string | null): Record<string, unknown> {
    const tracking: Record<string, unknown> = {};
    optional(tracking, "company", company);
    optional(tracking, "number", number);
    optional(tracking, "url", url);
    return tracking;
}

export function buildFulfillment(fulfillmentOrderIds: string[], tracking: Record<string, unknown> | null, notify: boolean): Record<string, unknown> {
    const orders: Record<string, unknown>[] = [];
    for (const id of fulfillmentOrderIds) orders.push({ fulfillmentOrderId: id });
    const fulfillment: Record<string, unknown> = { lineItemsByFulfillmentOrder: orders, notifyCustomer: notify };
    optional(fulfillment, "trackingInfo", tracking);
    return fulfillment;
}

export function buildVariantPrices(prices: PreparedVariantPrice[]): Record<string, unknown>[] {
    const variants: Record<string, unknown>[] = [];
    for (const price of prices) {
        const variant: Record<string, unknown> = { id: price.variantId, price: moneyText(price.price) };
        if (price.compareAtPrice !== null) variant["compareAtPrice"] = moneyText(price.compareAtPrice);
        variants.push(variant);
    }
    return variants;
}

// Shopify refuses this write if stock moved after `current` was read.
export function buildInventorySet(inventoryItemId: string, locationId: string, quantity: number, current: number, reason: string): Record<string, unknown> {
    return {
        name: "available",
        reason: reason,
        quantities: [{ inventoryItemId: inventoryItemId, locationId: locationId, quantity: quantity, changeFromQuantity: current }],
    };
}

// An explicit null skips the compare: a change by a delta cannot overwrite a newer count.
export function buildInventoryAdjust(inventoryItemId: string, locationId: string, delta: number, reason: string): Record<string, unknown> {
    return {
        name: "available",
        reason: reason,
        changes: [{ inventoryItemId: inventoryItemId, locationId: locationId, delta: delta, changeFromQuantity: null }],
    };
}

export function buildDiscount(
    title: string,
    code: string,
    percentage: number | null,
    amount: number | null,
    startsAt: string,
    endsAt: string | null,
    usageLimit: number | null,
    oncePerCustomer: boolean,
    minimumSubtotal: number | null,
): Record<string, unknown> {
    const value: Record<string, unknown> = {};
    if (percentage !== null) value["percentage"] = percentage / 100;
    if (amount !== null) value["discountAmount"] = { amount: moneyText(amount), appliesOnEachItem: false };
    const discount: Record<string, unknown> = {
        title: title,
        code: code,
        startsAt: startsAt,
        appliesOncePerCustomer: oncePerCustomer,
        context: { all: "ALL" },
        customerGets: { value: value, items: { all: true } },
    };
    optional(discount, "endsAt", endsAt);
    optional(discount, "usageLimit", usageLimit);
    if (minimumSubtotal !== null) {
        discount["minimumRequirement"] = { subtotal: { greaterThanOrEqualToSubtotal: moneyText(minimumSubtotal) } };
    }
    return discount;
}

export function buildDraftOrder(lines: PreparedDraftLine[], email: string | null, note: string | null, percentage: number | null, discountTitle: string | null): Record<string, unknown> {
    const lineItems: Record<string, unknown>[] = [];
    for (const line of lines) lineItems.push({ variantId: line.variantId, quantity: line.quantity });
    const input: Record<string, unknown> = { lineItems: lineItems };
    optional(input, "email", email);
    optional(input, "note", note);
    if (percentage !== null) {
        const discount: Record<string, unknown> = { value: percentage, valueType: "PERCENTAGE" };
        optional(discount, "title", discountTitle);
        input["appliedDiscount"] = discount;
    }
    return input;
}
