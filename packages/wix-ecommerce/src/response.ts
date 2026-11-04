// Response side of the Wix client: the shapes Wix returns, the package's own
// result types, and the mappers between them. Every raw field is optional,
// because Wix leaves out what a site does not use. Internal: `src/lib.ts`
// re-exports the result types.

import { validationError } from "./errors";

/** One page of results. */
export interface Page<T> {
    /** Items on this page. */
    items: T[];
    /** Pass this as `cursor` for the next page; null on the last page. */
    nextCursor: string | null;
}

/** One line of an order. */
export interface OrderLine {
    /** Line item id. */
    id: string;
    /** Item name. */
    name: string;
    /** Quantity ordered. */
    quantity: number;
    /** Unit price, in the order's currency; null when Wix sends none. */
    price: number | null;
    /** Catalog product id, or null for a custom line. */
    productId: string | null;
    /** Stock keeping unit, or null. */
    sku: string | null;
}

/** One order. */
export interface Order {
    /** Order id, for the other operations. */
    id: string;
    /** Order number shown in the dashboard. */
    number: string;
    /** `PENDING`, `APPROVED`, `CANCELED`, `REJECTED` or `INITIALIZED`. */
    status: string;
    /** Such as `PAID` or `PARTIALLY_REFUNDED`. */
    paymentStatus: string;
    /** `NOT_FULFILLED`, `PARTIALLY_FULFILLED` or `FULFILLED`. */
    fulfillmentStatus: string;
    /** Currency code. */
    currency: string;
    /** Total after discounts and tax. */
    total: number | null;
    /** Buyer's email, or null. */
    buyerEmail: string | null;
    /** Buyer's contact id, or null. */
    contactId: string | null;
    /** When the order was created. */
    createdDate: string;
    /** The order's lines. */
    lineItems: OrderLine[];
}

/** Tracking for one fulfillment. */
export interface Tracking {
    /** Tracking number. */
    number: string | null;
    /** Carrier. */
    provider: string | null;
    /** Tracking page. */
    link: string | null;
}

/** One fulfillment of an order. */
export interface Fulfillment {
    /** Fulfillment id. */
    id: string;
    /** When it was created. */
    createdDate: string;
    /** Tracking details, or null. */
    tracking: Tracking | null;
    /** Whether its lines count as fulfilled. */
    completed: boolean;
}

/** One payment on an order. */
export interface Payment {
    /** Payment id, for `refundOrder`. */
    id: string;
    /** Amount paid. */
    amount: number;
    /** Amount already refunded or being refunded from this payment. */
    refunded: number;
    /** Amount still available to refund. */
    refundable: number;
    /** Whether Wix allows refunds of this payment. */
    refundDisabled: boolean;
    /** When it was created. */
    createdDate: string;
}

/** The result of a refund. */
export interface Refund {
    /** Refund id. */
    id: string;
    /** `SUCCEEDED`, `PENDING`, `STARTED`, `SCHEDULED` or `FAILED`, for the refund's first transaction. */
    status: string;
}

/** An abandoned checkout. */
export interface AbandonedCheckout {
    /** Abandoned checkout id. */
    id: string;
    /** `ABANDONED` or `RECOVERED`. */
    status: string;
    /** Buyer's email, or null. */
    buyerEmail: string | null;
    /** Buyer's first name, or null. */
    firstName: string | null;
    /** Total, in `currency`; null when Wix sends none. */
    total: number | null;
    /** Currency code. */
    currency: string;
    /** Link the buyer can use to finish the checkout, or null in a list that leaves it out. */
    checkoutUrl: string | null;
    /** When the checkout was created. */
    createdDate: string;
}

/** A created cart. */
export interface Cart {
    /** Cart id, for `getCheckoutUrl`. */
    id: string;
    /** Number of lines in the cart. */
    lineCount: number;
}

/** One catalog product, on either catalog version. */
export interface Product {
    /** Product id. */
    id: string;
    /** Product name. */
    name: string;
    /** Slug in the product page address. */
    slug: string;
    /** Whether site visitors see it. */
    visible: boolean;
    /** Description, or null. */
    description: string | null;
    /** Price, or the lowest variant price; null when Wix sends none. */
    price: number | null;
    /** Currency code, or null. */
    currency: string | null;
    /** `V1_CATALOG` or `V3_CATALOG`. */
    catalog: string;
}

/** Stock of one variant. */
export interface InventoryLevel {
    /** Variant id. */
    variantId: string;
    /** Inventory item id. */
    inventoryItemId: string;
    /** Stores location, on catalog V3; otherwise null. */
    locationId: string | null;
    /** Quantity, when Wix tracks it; otherwise null. */
    quantity: number | null;
    /** Whether the variant can be bought. */
    inStock: boolean;
}

/** One contact. */
export interface Contact {
    /** Contact id. */
    id: string;
    /** Primary email, or null. */
    email: string | null;
    /** Primary phone, or null. */
    phone: string | null;
    /** First name, or null. */
    firstName: string | null;
    /** Last name, or null. */
    lastName: string | null;
    /** Company, or null. */
    company: string | null;
    /** When the contact was created. */
    createdDate: string;
}

export interface RawPrice {
    amount?: string | null;
}

export interface RawCursors {
    next?: string | null;
}

export interface RawPaging {
    hasNext?: boolean | null;
    cursors?: RawCursors | null;
}

export interface RawName {
    original?: string | null;
}

export interface RawCatalogReference {
    catalogItemId?: string | null;
}

export interface RawPhysicalProperties {
    sku?: string | null;
}

export interface RawLine {
    id?: string | null;
    productName?: RawName | null;
    quantity?: number | null;
    price?: RawPrice | null;
    catalogReference?: RawCatalogReference | null;
    physicalProperties?: RawPhysicalProperties | null;
}

export interface RawBuyer {
    email?: string | null;
    contactId?: string | null;
}

export interface RawPriceSummary {
    total?: RawPrice | null;
}

export interface RawOrder {
    id?: string | null;
    number?: string | number | null;
    status?: string | null;
    paymentStatus?: string | null;
    fulfillmentStatus?: string | null;
    currency?: string | null;
    priceSummary?: RawPriceSummary | null;
    buyerInfo?: RawBuyer | null;
    createdDate?: string | null;
    lineItems?: RawLine[] | null;
}

export interface OrderEnvelope {
    order?: RawOrder | null;
}

export interface OrderSearchResult {
    orders?: RawOrder[] | null;
    metadata?: RawPaging | null;
}

export interface RawTracking {
    trackingNumber?: string | null;
    shippingProvider?: string | null;
    trackingLink?: string | null;
}

export interface RawFulfillment {
    id?: string | null;
    createdDate?: string | null;
    trackingInfo?: RawTracking | null;
    completed?: boolean | null;
}

export interface RawOrderFulfillments {
    fulfillments?: RawFulfillment[] | null;
}

export interface FulfillmentsEnvelope {
    orderWithFulfillments?: RawOrderFulfillments | null;
    fulfillmentId?: string | null;
}

export interface RawPayment {
    id?: string | null;
    amount?: RawPrice | null;
    refundDisabled?: boolean | null;
    createdDate?: string | null;
}

export interface RawRefundTransaction {
    paymentId?: string | null;
    amount?: RawPrice | null;
    refundStatus?: string | null;
}

export interface RawRefund {
    id?: string | null;
    transactions?: RawRefundTransaction[] | null;
}

export interface RawOrderTransactions {
    payments?: RawPayment[] | null;
    refunds?: RawRefund[] | null;
}

export interface TransactionsEnvelope {
    orderTransactions?: RawOrderTransactions | null;
}

export interface RefundEnvelope {
    refund?: RawRefund | null;
}

export interface RawAbandonedCheckout {
    id?: string | null;
    status?: string | null;
    buyerInfo?: RawBuyer | null;
    contactDetails?: RawContactDetails | null;
    totalPrice?: RawPrice | null;
    currency?: string | null;
    checkoutUrl?: string | null;
    createdDate?: string | null;
}

export interface RawContactDetails {
    firstName?: string | null;
}

export interface AbandonedList {
    abandonedCheckouts?: RawAbandonedCheckout[] | null;
    metadata?: RawPaging | null;
    pagingMetadata?: RawPaging | null;
}

export interface AbandonedEnvelope {
    abandonedCheckout?: RawAbandonedCheckout | null;
}

export interface RawCart {
    id?: string | null;
    lineItems?: RawLine[] | null;
}

export interface CartEnvelope {
    cart?: RawCart | null;
}

export interface CheckoutUrlEnvelope {
    checkoutUrl?: string | null;
}

export interface VersionEnvelope {
    catalogVersion?: string | null;
}

export interface RawPriceData {
    price?: number | null;
    currency?: string | null;
}

export interface RawProductV1 {
    id?: string | null;
    name?: string | null;
    slug?: string | null;
    visible?: boolean | null;
    description?: string | null;
    priceData?: RawPriceData | null;
}

export interface ProductV1Envelope {
    product?: RawProductV1 | null;
}

export interface ProductV1List {
    products?: RawProductV1[] | null;
    totalResults?: number | null;
}

export interface RawPriceRange {
    minValue?: RawPrice | null;
}

export interface RawProductV3 {
    id?: string | null;
    revision?: string | null;
    name?: string | null;
    slug?: string | null;
    visible?: boolean | null;
    plainDescription?: string | null;
    currency?: string | null;
    actualPriceRange?: RawPriceRange | null;
}

export interface ProductV3Envelope {
    product?: RawProductV3 | null;
}

export interface ProductV3List {
    products?: RawProductV3[] | null;
    pagingMetadata?: RawPaging | null;
}

export interface RawInventoryVariantV1 {
    variantId?: string | null;
    quantity?: number | null;
    inStock?: boolean | null;
}

export interface RawInventoryItemV1 {
    id?: string | null;
    variants?: RawInventoryVariantV1[] | null;
}

export interface InventoryV1List {
    inventoryItems?: RawInventoryItemV1[] | null;
}

export interface RawInventoryItemV3 {
    id?: string | null;
    variantId?: string | null;
    locationId?: string | null;
    quantity?: number | null;
    inStock?: boolean | null;
    availabilityStatus?: string | null;
}

export interface InventoryV3List {
    inventoryItems?: RawInventoryItemV3[] | null;
}

export interface RawContactName {
    first?: string | null;
    last?: string | null;
}

export interface RawContactInfo {
    name?: RawContactName | null;
    company?: string | null;
}

export interface RawPrimaryInfo {
    email?: string | null;
    phone?: string | null;
}

export interface RawContact {
    id?: string | null;
    primaryInfo?: RawPrimaryInfo | null;
    info?: RawContactInfo | null;
    createdDate?: string | null;
}

export interface ContactEnvelope {
    contact?: RawContact | null;
}

export interface ContactList {
    contacts?: RawContact[] | null;
}

export interface RawErrorDetail {
    code?: string | null;
}

export interface RawErrorDetails {
    applicationError?: RawErrorDetail | null;
}

export interface RawErrorBody {
    message?: string | null;
    details?: RawErrorDetails | null;
}

function text(value: string | null | undefined): string {
    return value ?? "";
}

// A decimal amount from Wix, or null when it is absent or unreadable.
export function decimalOrNull(value: string | null | undefined): number | null {
    if (value === null || value === undefined || value.trim().length === 0) return null;
    const amount = Number(value);
    return Number.isFinite(amount) ? amount : null;
}

// A decimal amount a policy will compare. A missing amount must never read as
// zero, or a cap like `totalAmount <= 500` would pass for an order of any size.
export function requireAmount(value: string | null | undefined, name: string): number {
    const amount = decimalOrNull(value);
    if (amount === null) throw validationError("unexpected_response", "Wix sent no readable " + name);
    return amount;
}

export function orderLineFrom(raw: RawLine): OrderLine {
    return {
        id: text(raw.id),
        name: text(raw.productName?.original),
        quantity: raw.quantity ?? 0,
        price: decimalOrNull(raw.price?.amount),
        productId: raw.catalogReference?.catalogItemId ?? null,
        sku: raw.physicalProperties?.sku ?? null,
    };
}

export function orderFrom(raw: RawOrder): Order {
    const number = raw.number;
    const lines: OrderLine[] = [];
    for (const line of raw.lineItems ?? []) lines.push(orderLineFrom(line));
    return {
        id: text(raw.id),
        number: number === null || number === undefined ? "" : String(number),
        status: text(raw.status),
        paymentStatus: text(raw.paymentStatus),
        fulfillmentStatus: text(raw.fulfillmentStatus),
        currency: text(raw.currency),
        total: decimalOrNull(raw.priceSummary?.total?.amount),
        buyerEmail: raw.buyerInfo?.email ?? null,
        contactId: raw.buyerInfo?.contactId ?? null,
        createdDate: text(raw.createdDate),
        lineItems: lines,
    };
}

// The cursor for the next page, or null on the last page.
export function nextCursorFrom(paging: RawPaging | null | undefined): string | null {
    if (paging === null || paging === undefined) return null;
    const next = paging.cursors?.next ?? null;
    if (next === null || next.length === 0) return null;
    return paging.hasNext === false ? null : next;
}

export function fulfillmentFrom(raw: RawFulfillment): Fulfillment {
    const tracking = raw.trackingInfo;
    return {
        id: text(raw.id),
        createdDate: text(raw.createdDate),
        tracking: tracking === null || tracking === undefined
            ? null
            : { number: tracking.trackingNumber ?? null, provider: tracking.shippingProvider ?? null, link: tracking.trackingLink ?? null },
        completed: raw.completed ?? true,
    };
}

export function fulfillmentsFrom(envelope: FulfillmentsEnvelope): Fulfillment[] {
    const items: Fulfillment[] = [];
    for (const raw of envelope.orderWithFulfillments?.fulfillments ?? []) items.push(fulfillmentFrom(raw));
    return items;
}

// Add what each refund took from each payment. A failed refund took nothing.
function refundedByPayment(refunds: RawRefund[]): Map<string, number> {
    const refunded = new Map<string, number>();
    for (const refund of refunds) {
        for (const transaction of refund.transactions ?? []) {
            if (transaction.refundStatus === "FAILED") continue;
            const id = text(transaction.paymentId);
            refunded.set(id, (refunded.get(id) ?? 0) + requireAmount(transaction.amount?.amount, "refund amount"));
        }
    }
    return refunded;
}

export function paymentsFrom(envelope: TransactionsEnvelope): Payment[] {
    const transactions = envelope.orderTransactions;
    const refunded = refundedByPayment(transactions?.refunds ?? []);
    const payments: Payment[] = [];
    for (const raw of transactions?.payments ?? []) {
        const id = text(raw.id);
        const amount = requireAmount(raw.amount?.amount, "payment amount");
        const taken = refunded.get(id) ?? 0;
        payments.push({
            id: id,
            amount: amount,
            refunded: taken,
            // Rounded so that binary fractions cannot refuse a full refund by a hair.
            refundable: Math.max(0, Math.round((amount - taken) * 1000) / 1000),
            refundDisabled: raw.refundDisabled ?? false,
            createdDate: text(raw.createdDate),
        });
    }
    return payments;
}

export function refundFrom(raw: RawRefund | null | undefined): Refund {
    if (raw === null || raw === undefined) throw validationError("unexpected_response", "Wix sent no refund");
    let status = "";
    for (const transaction of raw.transactions ?? []) {
        status = text(transaction.refundStatus);
        break;
    }
    return { id: text(raw.id), status: status };
}

export function abandonedFrom(raw: RawAbandonedCheckout): AbandonedCheckout {
    return {
        id: text(raw.id),
        status: text(raw.status),
        buyerEmail: raw.buyerInfo?.email ?? null,
        firstName: raw.contactDetails?.firstName ?? null,
        total: decimalOrNull(raw.totalPrice?.amount),
        currency: text(raw.currency),
        checkoutUrl: raw.checkoutUrl ?? null,
        createdDate: text(raw.createdDate),
    };
}

export function cartFrom(envelope: CartEnvelope): Cart {
    const cart = envelope.cart;
    if (cart === null || cart === undefined) throw validationError("unexpected_response", "Wix sent no cart");
    return { id: text(cart.id), lineCount: (cart.lineItems ?? []).length };
}

export function productFromV1(raw: RawProductV1): Product {
    return {
        id: text(raw.id),
        name: text(raw.name),
        slug: text(raw.slug),
        visible: raw.visible ?? false,
        description: raw.description ?? null,
        price: raw.priceData?.price ?? null,
        currency: raw.priceData?.currency ?? null,
        catalog: "V1_CATALOG",
    };
}

export function productFromV3(raw: RawProductV3): Product {
    return {
        id: text(raw.id),
        name: text(raw.name),
        slug: text(raw.slug),
        visible: raw.visible ?? false,
        description: raw.plainDescription ?? null,
        price: decimalOrNull(raw.actualPriceRange?.minValue?.amount),
        currency: raw.currency ?? null,
        catalog: "V3_CATALOG",
    };
}

export function inventoryFromV1(list: InventoryV1List): InventoryLevel[] {
    const levels: InventoryLevel[] = [];
    for (const item of list.inventoryItems ?? []) {
        for (const variant of item.variants ?? []) {
            levels.push({
                variantId: text(variant.variantId),
                inventoryItemId: text(item.id),
                locationId: null,
                quantity: variant.quantity ?? null,
                inStock: variant.inStock ?? false,
            });
        }
    }
    return levels;
}

export function inventoryFromV3(list: InventoryV3List): InventoryLevel[] {
    const levels: InventoryLevel[] = [];
    for (const item of list.inventoryItems ?? []) {
        const quantity = item.quantity ?? null;
        levels.push({
            variantId: text(item.variantId),
            inventoryItemId: text(item.id),
            locationId: item.locationId ?? null,
            quantity: quantity,
            inStock: item.inStock ?? (quantity !== null ? quantity > 0 : item.availabilityStatus === "IN_STOCK"),
        });
    }
    return levels;
}

export function contactFrom(raw: RawContact): Contact {
    return {
        id: text(raw.id),
        email: raw.primaryInfo?.email ?? null,
        phone: raw.primaryInfo?.phone ?? null,
        firstName: raw.info?.name?.first ?? null,
        lastName: raw.info?.name?.last ?? null,
        company: raw.info?.company ?? null,
        createdDate: text(raw.createdDate),
    };
}

function errorCodeFor(status: number): string {
    if (status === 400) return "invalid_request";
    if (status === 401) return "unauthorized";
    if (status === 403) return "forbidden";
    if (status === 404) return "not_found";
    if (status === 409) return "conflict";
    if (status === 428) return "precondition_failed";
    if (status === 429) return "rate_limited";
    if (status >= 500) return "server_error";
    return "http_" + status.toString();
}

function parseErrorBody(body: string): RawErrorBody {
    try {
        return JSON.parse(body) as RawErrorBody;
    } catch (error) {
        return {};
    }
}

// Wix's detail when the body has one, and a code either way.
export function failureFrom(status: number, statusText: string, body: string): { code: string; message: string } {
    const parsed = parseErrorBody(body);
    const own = parsed.details?.applicationError?.code ?? "";
    const code = own.length > 0 ? own : errorCodeFor(status);
    const detail = parsed.message ?? "";
    const message = detail.length > 0
        ? "Wix " + status.toString() + " " + code + ": " + detail
        : "Wix request failed: HTTP " + status.toString() + " " + statusText;
    return { code: code, message: message };
}

// Seconds from a `Retry-After` header of digits, or null.
export function retryAfterFrom(header: string): number | null {
    const value = header.trim();
    if (value.length === 0) return null;
    const seconds = Number(value);
    return Number.isSafeInteger(seconds) && seconds >= 0 ? seconds : null;
}
