// Response side of the Shopify client: the shapes Shopify returns, the package's
// own result types, and the mappers between them. Internal: `src/lib.ts`
// re-exports the result types. Money is the buyer's currency throughout, because
// that is the currency a refund is made in.

import { ShopifyError, validationError } from "./errors";

/** One page of results. */
export interface Page<T> {
    /** Items on this page. */
    items: T[];
    /** Pass this as `after` for the next page; null on the last page. */
    nextCursor: string | null;
}

/** One order in a list. */
export interface OrderSummary {
    /** Order id, for the other operations. */
    id: string;
    /** Order name shown in the admin, such as `#1001`. */
    name: string;
    /** When the order was created. */
    createdAt: string;
    /** When it was canceled, or null. */
    cancelledAt: string | null;
    /** Such as `PAID` or `PARTIALLY_REFUNDED`, or null. */
    financialStatus: string | null;
    /** Such as `UNFULFILLED` or `FULFILLED`. */
    fulfillmentStatus: string;
    /** The buyer's currency. */
    currency: string;
    /** Current total, after refunds and edits; null when Shopify sends none. */
    total: number | null;
    /** Buyer's email, or null. */
    email: string | null;
}

/** One line of an order. */
export interface OrderLine {
    /** Line item id. */
    id: string;
    /** Item title. */
    title: string;
    /** Quantity ordered. */
    quantity: number;
    /** Quantity that can still be refunded. */
    refundableQuantity: number;
    /** Quantity not yet fulfilled. */
    unfulfilledQuantity: number;
    /** Stock keeping unit, or null. */
    sku: string | null;
    /** Variant id, or null for a custom line. */
    variantId: string | null;
    /** Unit price after discounts; null when Shopify sends none. */
    unitPrice: number | null;
}

/** One order, with its lines. */
export interface Order extends OrderSummary {
    /** Note for staff, or null. */
    note: string | null;
    /** Tags. */
    tags: string[];
    /** The first 100 lines. */
    lineItems: OrderLine[];
    /** True when the order has more than these lines. */
    moreLineItems: boolean;
}

/** A started cancellation. Shopify finishes it in the background. */
export interface CancelJob {
    /** Job id. */
    jobId: string;
    /** Whether it has finished. */
    done: boolean;
}

/** One payment transaction a refund would make. */
export interface SuggestedTransaction {
    /** Payment gateway. */
    gateway: string;
    /** Transaction being refunded, or null. */
    parentTransactionId: string | null;
    /** Amount, in `currency`. */
    amount: number;
    /** The same amount, exactly as Shopify sent it. */
    amountText: string;
}

/** What a refund would return. */
export interface SuggestedRefund {
    /** Total of the transactions. */
    amount: number;
    /** The buyer's currency. */
    currency: string;
    /** Most that can be refunded; null when Shopify sends none. */
    maximumRefundable: number | null;
    /** The payment transactions. */
    transactions: SuggestedTransaction[];
}

/** A made refund. */
export interface Refund {
    /** Refund id. */
    id: string;
    /** Amount refunded; null when Shopify sends none. */
    amount: number | null;
    /** Currency of the amount, or an empty string. */
    currency: string;
}

/** A hold on a fulfillment order. */
export interface FulfillmentHold {
    /** Hold id, for `releaseFulfillmentOrderHold`. */
    id: string;
    /** Such as `AWAITING_PAYMENT`. */
    reason: string;
}

/** One line of a fulfillment order. */
export interface FulfillmentOrderLine {
    /** Fulfillment order line id. */
    id: string;
    /** Product title. */
    title: string;
    /** Quantity left to fulfill. */
    remainingQuantity: number;
    /** Quantity in total. */
    totalQuantity: number;
}

/** The work of shipping part of an order from one location. */
export interface FulfillmentOrder {
    /** Fulfillment order id. */
    id: string;
    /** Such as `OPEN`, `IN_PROGRESS`, `ON_HOLD`, `CLOSED`. */
    status: string;
    /** Assigned location's name. */
    locationName: string;
    /** Assigned location's id, or null if it was deleted. */
    locationId: string | null;
    /** Holds on it. */
    holds: FulfillmentHold[];
    /** The first 20 lines. */
    lines: FulfillmentOrderLine[];
    /** True when it has more than these lines. */
    moreLines: boolean;
}

/** Tracking for one shipment. */
export interface Tracking {
    /** Carrier, or null. */
    company: string | null;
    /** Tracking number, or null. */
    number: string | null;
    /** Tracking page, or null. */
    url: string | null;
}

/** A recorded fulfillment. */
export interface Fulfillment {
    /** Fulfillment id. */
    id: string;
    /** Such as `SUCCESS`. */
    status: string;
    /** Tracking details. */
    tracking: Tracking[];
}

/** A fulfillment order after a hold or a release. */
export interface FulfillmentOrderState {
    /** Fulfillment order id. */
    id: string;
    /** Its status now. */
    status: string;
    /** The new hold's id, or null after a release. */
    holdId: string | null;
}

/** One product in a list. */
export interface ProductSummary {
    /** Product id. */
    id: string;
    /** Title. */
    title: string;
    /** Slug in the product page address. */
    handle: string;
    /** `ACTIVE`, `ARCHIVED`, `DRAFT` or `UNLISTED`. */
    status: string;
    /** Vendor. */
    vendor: string;
    /** Product type. */
    productType: string;
    /** Tags. */
    tags: string[];
    /** Stock across every variant and location. */
    totalInventory: number;
    /** Lowest variant price; null when Shopify sends none. */
    minPrice: number | null;
    /** Currency of that price, or null. */
    currency: string | null;
}

/** One variant of a product. */
export interface Variant {
    /** Variant id. */
    id: string;
    /** Title. */
    title: string;
    /** Stock keeping unit, or null. */
    sku: string | null;
    /** Price; null when Shopify sends none. */
    price: number | null;
    /** Compare-at price, or null. */
    compareAtPrice: number | null;
    /** Stock, or null. */
    inventoryQuantity: number | null;
}

/** One product, with its variants. */
export interface Product extends ProductSummary {
    /** Description, as HTML. */
    descriptionHtml: string;
    /** The first 100 variants. */
    variants: Variant[];
    /** True when the product has more than these variants. */
    moreVariants: boolean;
}

/** One location. */
export interface Location {
    /** Location id. */
    id: string;
    /** Name. */
    name: string;
    /** Whether it is active. */
    isActive: boolean;
    /** Whether it fulfills online orders. */
    fulfillsOnlineOrders: boolean;
}

/** Stock of one variant at one location. */
export interface InventoryLevel {
    /** Location id. */
    locationId: string;
    /** Location name. */
    locationName: string;
    /** Quantity available to sell, or null. */
    available: number | null;
    /** Quantity held for orders, or null. */
    committed: number | null;
    /** Quantity physically present, or null. */
    onHand: number | null;
}

/** Stock of one variant. */
export interface VariantInventory {
    /** Variant id. */
    variantId: string;
    /** Variant title. */
    title: string;
    /** Inventory item id, for `setInventory` and `adjustInventory`. */
    inventoryItemId: string;
    /** Stock at up to 10 locations. */
    levels: InventoryLevel[];
}

/** One customer in a list. */
export interface CustomerSummary {
    /** Customer id. */
    id: string;
    /** First name, or null. */
    firstName: string | null;
    /** Last name, or null. */
    lastName: string | null;
    /** Default email, or null. */
    email: string | null;
    /** Default phone, or null. */
    phone: string | null;
    /** Number of orders. */
    numberOfOrders: number;
}

/** One customer, with their recent orders. */
export interface Customer extends CustomerSummary {
    /** Total spent; null when Shopify sends none. */
    amountSpent: number | null;
    /** Currency of that total, or null. */
    currency: string | null;
    /** The last 10 orders, newest first. */
    orders: OrderSummary[];
}

/** An abandoned checkout. */
export interface AbandonedCheckout {
    /** Abandoned checkout id. */
    id: string;
    /** Name, such as `#1001`. */
    name: string;
    /** When it was created. */
    createdAt: string;
    /** When it was completed, or null. */
    completedAt: string | null;
    /** Link the buyer can use to finish it. */
    url: string;
    /** Buyer's email, or null. */
    email: string | null;
    /** Total; null when Shopify sends none. */
    total: number | null;
    /** Currency of the total, or an empty string. */
    currency: string;
}

/** A discount code. */
export interface DiscountCode {
    /** Discount id, for `deactivateDiscountCode`. */
    id: string;
}

/** A draft order. */
export interface DraftOrder {
    /** Draft order id. */
    id: string;
    /** Name, such as `#D1`. */
    name: string;
    /** Such as `OPEN`. */
    status: string;
    /** Link the buyer can use to pay, or null. */
    invoiceUrl: string | null;
    /** Total; null when Shopify sends none. */
    total: number | null;
    /** Currency of the total, or an empty string. */
    currency: string;
}

/** The store. */
export interface Shop {
    /** Store name. */
    name: string;
    /** Contact email. */
    email: string;
    /** The store's currency. */
    currency: string;
    /** Time zone. */
    timezone: string;
    /** Its `myshopify.com` domain. */
    domain: string;
    /** Its plan. */
    plan: string;
}

export interface RawMoney {
    amount?: string | null;
    currencyCode?: string | null;
}

export interface RawMoneyBag {
    presentmentMoney?: RawMoney | null;
}

export interface RawPageInfo {
    hasNextPage?: boolean | null;
    endCursor?: string | null;
}

export interface RawId {
    id?: string | null;
}

export interface RawOrder {
    id?: string | null;
    name?: string | null;
    createdAt?: string | null;
    cancelledAt?: string | null;
    displayFinancialStatus?: string | null;
    displayFulfillmentStatus?: string | null;
    presentmentCurrencyCode?: string | null;
    email?: string | null;
    currentTotalPriceSet?: RawMoneyBag | null;
    note?: string | null;
    tags?: string[] | null;
    lineItems?: RawOrderLines | null;
}

export interface RawOrderLine {
    id?: string | null;
    title?: string | null;
    quantity?: number | null;
    refundableQuantity?: number | null;
    unfulfilledQuantity?: number | null;
    sku?: string | null;
    variant?: RawId | null;
    discountedUnitPriceSet?: RawMoneyBag | null;
}

export interface RawOrderLines {
    nodes?: RawOrderLine[] | null;
    pageInfo?: RawPageInfo | null;
}

export interface RawOrderList {
    nodes?: RawOrder[] | null;
    pageInfo?: RawPageInfo | null;
}

export interface OrdersData {
    orders?: RawOrderList | null;
}

export interface OrderData {
    order?: RawOrder | null;
}

export interface RawUserError {
    field?: string[] | null;
    message?: string | null;
    code?: string | null;
}

export interface RawOrderUpdate {
    order?: RawOrder | null;
    userErrors?: RawUserError[] | null;
}

export interface OrderUpdateData {
    orderUpdate?: RawOrderUpdate | null;
}

export interface RawJob {
    id?: string | null;
    done?: boolean | null;
}

export interface RawOrderCancel {
    job?: RawJob | null;
    orderCancelUserErrors?: RawUserError[] | null;
}

export interface OrderCancelData {
    orderCancel?: RawOrderCancel | null;
}

export interface RawSuggestedTransaction {
    gateway?: string | null;
    amountSet?: RawMoneyBag | null;
    parentTransaction?: RawId | null;
}

export interface RawSuggestedRefund {
    amountSet?: RawMoneyBag | null;
    maximumRefundableSet?: RawMoneyBag | null;
    suggestedTransactions?: RawSuggestedTransaction[] | null;
}

export interface RawOrderSuggestion {
    id?: string | null;
    suggestedRefund?: RawSuggestedRefund | null;
}

export interface SuggestionData {
    order?: RawOrderSuggestion | null;
}

export interface RawRefund {
    id?: string | null;
    totalRefundedSet?: RawMoneyBag | null;
}

export interface RawRefundCreate {
    refund?: RawRefund | null;
    userErrors?: RawUserError[] | null;
}

export interface RefundCreateData {
    refundCreate?: RawRefundCreate | null;
}

export interface RawAssignedLocation {
    name?: string | null;
    location?: RawId | null;
}

export interface RawHold {
    id?: string | null;
    reason?: string | null;
}

export interface RawFulfillmentOrderLine {
    id?: string | null;
    productTitle?: string | null;
    remainingQuantity?: number | null;
    totalQuantity?: number | null;
}

export interface RawFulfillmentOrderLines {
    nodes?: RawFulfillmentOrderLine[] | null;
    pageInfo?: RawPageInfo | null;
}

export interface RawFulfillmentOrder {
    id?: string | null;
    status?: string | null;
    assignedLocation?: RawAssignedLocation | null;
    fulfillmentHolds?: RawHold[] | null;
    lineItems?: RawFulfillmentOrderLines | null;
}

export interface RawFulfillmentOrders {
    nodes?: RawFulfillmentOrder[] | null;
}

export interface RawOrderFulfillmentOrders {
    id?: string | null;
    fulfillmentOrders?: RawFulfillmentOrders | null;
}

export interface FulfillmentOrdersData {
    order?: RawOrderFulfillmentOrders | null;
}

export interface RawTracking {
    company?: string | null;
    number?: string | null;
    url?: string | null;
}

export interface RawFulfillment {
    id?: string | null;
    status?: string | null;
    trackingInfo?: RawTracking[] | null;
}

export interface RawFulfillmentCreate {
    fulfillment?: RawFulfillment | null;
    userErrors?: RawUserError[] | null;
}

export interface FulfillmentCreateData {
    fulfillmentCreate?: RawFulfillmentCreate | null;
}

export interface FulfillmentTrackingData {
    fulfillmentTrackingInfoUpdate?: RawFulfillmentCreate | null;
}

export interface RawFulfillmentOwner {
    id?: string | null;
    order?: RawId | null;
}

export interface FulfillmentOwnerData {
    fulfillment?: RawFulfillmentOwner | null;
}

export interface RawHoldResult {
    fulfillmentHold?: RawHold | null;
    fulfillmentOrder?: RawFulfillmentOrder | null;
    userErrors?: RawUserError[] | null;
}

export interface HoldData {
    fulfillmentOrderHold?: RawHoldResult | null;
}

export interface ReleaseData {
    fulfillmentOrderReleaseHold?: RawHoldResult | null;
}

export interface RawMinPrice {
    minVariantPrice?: RawMoney | null;
}

export interface RawVariant {
    id?: string | null;
    title?: string | null;
    sku?: string | null;
    price?: string | null;
    compareAtPrice?: string | null;
    inventoryQuantity?: number | null;
}

export interface RawVariants {
    nodes?: RawVariant[] | null;
    pageInfo?: RawPageInfo | null;
}

export interface RawProduct {
    id?: string | null;
    title?: string | null;
    handle?: string | null;
    status?: string | null;
    vendor?: string | null;
    productType?: string | null;
    tags?: string[] | null;
    totalInventory?: number | null;
    priceRangeV2?: RawMinPrice | null;
    descriptionHtml?: string | null;
    variants?: RawVariants | null;
}

export interface RawProductList {
    nodes?: RawProduct[] | null;
    pageInfo?: RawPageInfo | null;
}

export interface ProductsData {
    products?: RawProductList | null;
}

export interface ProductData {
    product?: RawProduct | null;
}

export interface RawProductUpdate {
    product?: RawProduct | null;
    userErrors?: RawUserError[] | null;
}

export interface ProductUpdateData {
    productUpdate?: RawProductUpdate | null;
}

export interface RawVariantsUpdate {
    productVariants?: RawVariant[] | null;
    userErrors?: RawUserError[] | null;
}

export interface VariantsUpdateData {
    productVariantsBulkUpdate?: RawVariantsUpdate | null;
}

export interface RawLocation {
    id?: string | null;
    name?: string | null;
    isActive?: boolean | null;
    fulfillsOnlineOrders?: boolean | null;
}

export interface RawLocations {
    nodes?: RawLocation[] | null;
}

export interface LocationsData {
    locations?: RawLocations | null;
}

export interface RawQuantity {
    name?: string | null;
    quantity?: number | null;
}

export interface RawLevelLocation {
    id?: string | null;
    name?: string | null;
}

export interface RawLevel {
    location?: RawLevelLocation | null;
    quantities?: RawQuantity[] | null;
}

export interface RawLevels {
    nodes?: RawLevel[] | null;
}

export interface RawInventoryItem {
    id?: string | null;
    inventoryLevels?: RawLevels | null;
    inventoryLevel?: RawLevel | null;
}

export interface RawInventoryVariant {
    id?: string | null;
    title?: string | null;
    inventoryItem?: RawInventoryItem | null;
}

export interface RawInventoryVariants {
    nodes?: RawInventoryVariant[] | null;
    pageInfo?: RawPageInfo | null;
}

export interface RawInventoryProduct {
    id?: string | null;
    variants?: RawInventoryVariants | null;
}

export interface InventoryLevelsData {
    product?: RawInventoryProduct | null;
}

export interface AvailableData {
    inventoryItem?: RawInventoryItem | null;
}

export interface RawInventoryWrite {
    userErrors?: RawUserError[] | null;
}

export interface InventorySetData {
    inventorySetQuantities?: RawInventoryWrite | null;
}

export interface InventoryAdjustData {
    inventoryAdjustQuantities?: RawInventoryWrite | null;
}

export interface RawEmailAddress {
    emailAddress?: string | null;
}

export interface RawPhoneNumber {
    phoneNumber?: string | null;
}

export interface RawCustomerOrders {
    nodes?: RawOrder[] | null;
}

export interface RawCustomer {
    id?: string | null;
    firstName?: string | null;
    lastName?: string | null;
    defaultEmailAddress?: RawEmailAddress | null;
    defaultPhoneNumber?: RawPhoneNumber | null;
    numberOfOrders?: string | null;
    amountSpent?: RawMoney | null;
    orders?: RawCustomerOrders | null;
}

export interface RawCustomerList {
    nodes?: RawCustomer[] | null;
    pageInfo?: RawPageInfo | null;
}

export interface CustomersData {
    customers?: RawCustomerList | null;
}

export interface CustomerData {
    customer?: RawCustomer | null;
}

export interface RawAbandonedCustomer {
    defaultEmailAddress?: RawEmailAddress | null;
}

export interface RawAbandonedCheckout {
    id?: string | null;
    name?: string | null;
    createdAt?: string | null;
    completedAt?: string | null;
    abandonedCheckoutUrl?: string | null;
    customer?: RawAbandonedCustomer | null;
    totalPriceSet?: RawMoneyBag | null;
}

export interface RawAbandonedList {
    nodes?: RawAbandonedCheckout[] | null;
    pageInfo?: RawPageInfo | null;
}

export interface AbandonedData {
    abandonedCheckouts?: RawAbandonedList | null;
}

export interface RawDiscountResult {
    codeDiscountNode?: RawId | null;
    userErrors?: RawUserError[] | null;
}

export interface DiscountCreateData {
    discountCodeBasicCreate?: RawDiscountResult | null;
}

export interface DiscountDeactivateData {
    discountCodeDeactivate?: RawDiscountResult | null;
}

export interface RawDraftOrder {
    id?: string | null;
    name?: string | null;
    status?: string | null;
    invoiceUrl?: string | null;
    totalPriceSet?: RawMoneyBag | null;
}

export interface RawDraftResult {
    draftOrder?: RawDraftOrder | null;
    userErrors?: RawUserError[] | null;
}

export interface DraftCreateData {
    draftOrderCreate?: RawDraftResult | null;
}

export interface DraftSendData {
    draftOrderInvoiceSend?: RawDraftResult | null;
}

export interface RawShopPlan {
    publicDisplayName?: string | null;
}

export interface RawShop {
    name?: string | null;
    email?: string | null;
    currencyCode?: string | null;
    ianaTimezone?: string | null;
    myshopifyDomain?: string | null;
    plan?: RawShopPlan | null;
}

export interface ShopData {
    shop?: RawShop | null;
}

export interface RawThrottleStatus {
    currentlyAvailable?: number | null;
    restoreRate?: number | null;
}

export interface RawCost {
    requestedQueryCost?: number | null;
    throttleStatus?: RawThrottleStatus | null;
}

export interface RawExtensions {
    cost?: RawCost | null;
}

export interface RawErrorExtensions {
    code?: string | null;
}

export interface RawGraphQlError {
    message?: string | null;
    extensions?: RawErrorExtensions | null;
}

export interface GraphQlStatus {
    errors?: RawGraphQlError[] | null;
    extensions?: RawExtensions | null;
}

export interface GraphQlResponse<T> {
    data?: T | null;
}

export interface HttpErrorBody {
    errors?: string | null;
}

function text(value: string | null | undefined): string {
    return value ?? "";
}

// A decimal amount from Shopify, or null when it is absent or unreadable.
export function decimalOrNull(value: string | null | undefined): number | null {
    if (value === null || value === undefined || value.trim().length === 0) return null;
    const amount = Number(value);
    return Number.isFinite(amount) ? amount : null;
}

// A decimal amount a policy will compare. A missing amount must never read as
// zero, or a cap like `totalAmount <= 500` would pass for an order of any size.
export function requireAmount(value: string | null | undefined, name: string): number {
    const amount = decimalOrNull(value);
    if (amount === null) throw validationError("unexpected_response", "Shopify sent no readable " + name);
    return amount;
}

export function requireId(value: string | null | undefined, name: string): string {
    const id = text(value);
    if (id.length === 0) throw validationError("unexpected_response", "Shopify sent no " + name);
    return id;
}

function moneyAmount(bag: RawMoneyBag | null | undefined): number | null {
    return decimalOrNull(bag?.presentmentMoney?.amount);
}

function moneyCurrency(bag: RawMoneyBag | null | undefined): string {
    return text(bag?.presentmentMoney?.currencyCode);
}

// The cursor for the next page, or null on the last page.
export function nextCursorFrom(pageInfo: RawPageInfo | null | undefined): string | null {
    if (pageInfo?.hasNextPage !== true) return null;
    const cursor = pageInfo.endCursor ?? "";
    return cursor.length === 0 ? null : cursor;
}

export function orderSummaryFrom(raw: RawOrder): OrderSummary {
    return {
        id: text(raw.id),
        name: text(raw.name),
        createdAt: text(raw.createdAt),
        cancelledAt: raw.cancelledAt ?? null,
        financialStatus: raw.displayFinancialStatus ?? null,
        fulfillmentStatus: text(raw.displayFulfillmentStatus),
        currency: text(raw.presentmentCurrencyCode),
        total: moneyAmount(raw.currentTotalPriceSet),
        email: raw.email ?? null,
    };
}

export function orderFrom(raw: RawOrder): Order {
    const summary = orderSummaryFrom(raw);
    const lines: OrderLine[] = [];
    for (const line of raw.lineItems?.nodes ?? []) {
        lines.push({
            id: text(line.id),
            title: text(line.title),
            quantity: line.quantity ?? 0,
            refundableQuantity: line.refundableQuantity ?? 0,
            unfulfilledQuantity: line.unfulfilledQuantity ?? 0,
            sku: line.sku ?? null,
            variantId: line.variant?.id ?? null,
            unitPrice: moneyAmount(line.discountedUnitPriceSet),
        });
    }
    return {
        id: summary.id,
        name: summary.name,
        createdAt: summary.createdAt,
        cancelledAt: summary.cancelledAt,
        financialStatus: summary.financialStatus,
        fulfillmentStatus: summary.fulfillmentStatus,
        currency: summary.currency,
        total: summary.total,
        email: summary.email,
        note: raw.note ?? null,
        tags: raw.tags ?? [],
        lineItems: lines,
        moreLineItems: raw.lineItems?.pageInfo?.hasNextPage === true,
    };
}

// Append a note, so what staff wrote before stays. `orderUpdate` replaces the field.
export function appendNote(existing: string | null | undefined, note: string): string {
    const current = text(existing);
    return current.length === 0 ? note : current + "\n" + note;
}

// Add tags to the ones already there, without repeats, ignoring case. `orderUpdate` replaces the list.
export function mergeTags(existing: string[], added: string[]): string[] {
    const tags: string[] = [];
    const seen: string[] = [];
    for (const list of [existing, added]) {
        for (const tag of list) {
            const key = tag.toLowerCase();
            if (seen.includes(key)) continue;
            seen.push(key);
            tags.push(tag);
        }
    }
    return tags;
}

export function cancelJobFrom(raw: RawOrderCancel | null | undefined): CancelJob {
    requireNoUserErrors(raw?.orderCancelUserErrors);
    return { jobId: requireId(raw?.job?.id, "cancellation job"), done: raw?.job?.done ?? false };
}

// The transactions Shopify proposes, summed in the buyer's currency. These exact
// amounts go in the refund, so the amount checked is the amount refunded.
export function suggestedRefundFrom(raw: RawSuggestedRefund): SuggestedRefund {
    const transactions: SuggestedTransaction[] = [];
    let total = 0;
    let currency = moneyCurrency(raw.amountSet);
    for (const item of raw.suggestedTransactions ?? []) {
        const amountText = text(item.amountSet?.presentmentMoney?.amount);
        const amount = requireAmount(amountText, "refund amount");
        const gateway = text(item.gateway);
        if (gateway.length === 0) throw validationError("unexpected_response", "Shopify sent no payment gateway");
        total = total + amount;
        if (currency.length === 0) currency = moneyCurrency(item.amountSet);
        transactions.push({ gateway: gateway, parentTransactionId: item.parentTransaction?.id ?? null, amount: amount, amountText: amountText });
    }
    return {
        amount: Math.round(total * 1000) / 1000,
        currency: currency,
        maximumRefundable: moneyAmount(raw.maximumRefundableSet),
        transactions: transactions,
    };
}

export function refundFrom(raw: RawRefundCreate | null | undefined): Refund {
    requireNoUserErrors(raw?.userErrors);
    const refund = raw?.refund;
    if (refund === null || refund === undefined) throw validationError("unexpected_response", "Shopify sent no refund");
    return { id: text(refund.id), amount: moneyAmount(refund.totalRefundedSet), currency: moneyCurrency(refund.totalRefundedSet) };
}

export function fulfillmentOrderFrom(raw: RawFulfillmentOrder): FulfillmentOrder {
    const holds: FulfillmentHold[] = [];
    for (const hold of raw.fulfillmentHolds ?? []) holds.push({ id: text(hold.id), reason: text(hold.reason) });
    const lines: FulfillmentOrderLine[] = [];
    for (const line of raw.lineItems?.nodes ?? []) {
        lines.push({
            id: text(line.id),
            title: text(line.productTitle),
            remainingQuantity: line.remainingQuantity ?? 0,
            totalQuantity: line.totalQuantity ?? 0,
        });
    }
    return {
        id: text(raw.id),
        status: text(raw.status),
        locationName: text(raw.assignedLocation?.name),
        locationId: raw.assignedLocation?.location?.id ?? null,
        holds: holds,
        lines: lines,
        moreLines: raw.lineItems?.pageInfo?.hasNextPage === true,
    };
}

// The ids to fulfill: the caller's, each of which must be open on this order, or
// every open one. The order in the check is the order the ids belong to.
export function chooseFulfillmentOrders(raw: RawFulfillmentOrder[], chosen: string[]): string[] {
    const open: string[] = [];
    const locations: string[] = [];
    for (const order of raw) {
        const status = text(order.status);
        const id = text(order.id);
        const selected = chosen.length === 0 || chosen.includes(id);
        if (!selected || (status !== "OPEN" && status !== "IN_PROGRESS")) continue;
        open.push(id);
        const location = text(order.assignedLocation?.location?.id);
        if (!locations.includes(location)) locations.push(location);
    }
    for (const id of chosen) {
        if (!open.includes(id)) throw validationError("fulfillment_order_not_open", id + " is not an open fulfillment order of this order");
    }
    if (open.length === 0) throw validationError("nothing_to_fulfill", "this order has no open fulfillment orders");
    if (locations.length > 1) throw validationError("several_locations", "these fulfillment orders are at different locations; name the ones to fulfill together");
    return open;
}

export function fulfillmentFrom(raw: RawFulfillmentCreate | null | undefined): Fulfillment {
    requireNoUserErrors(raw?.userErrors);
    const fulfillment = raw?.fulfillment;
    if (fulfillment === null || fulfillment === undefined) throw validationError("unexpected_response", "Shopify sent no fulfillment");
    const tracking: Tracking[] = [];
    for (const item of fulfillment.trackingInfo ?? []) {
        tracking.push({ company: item.company ?? null, number: item.number ?? null, url: item.url ?? null });
    }
    return { id: text(fulfillment.id), status: text(fulfillment.status), tracking: tracking };
}

export function fulfillmentOrderStateFrom(raw: RawHoldResult | null | undefined): FulfillmentOrderState {
    requireNoUserErrors(raw?.userErrors);
    const order = raw?.fulfillmentOrder;
    if (order === null || order === undefined) throw validationError("unexpected_response", "Shopify sent no fulfillment order");
    return { id: text(order.id), status: text(order.status), holdId: raw.fulfillmentHold?.id ?? null };
}

export function productSummaryFrom(raw: RawProduct): ProductSummary {
    const price = raw.priceRangeV2?.minVariantPrice;
    return {
        id: text(raw.id),
        title: text(raw.title),
        handle: text(raw.handle),
        status: text(raw.status),
        vendor: text(raw.vendor),
        productType: text(raw.productType),
        tags: raw.tags ?? [],
        totalInventory: raw.totalInventory ?? 0,
        minPrice: decimalOrNull(price?.amount),
        currency: price?.currencyCode ?? null,
    };
}

export function variantFrom(raw: RawVariant): Variant {
    return {
        id: text(raw.id),
        title: text(raw.title),
        sku: raw.sku ?? null,
        price: decimalOrNull(raw.price),
        compareAtPrice: decimalOrNull(raw.compareAtPrice),
        inventoryQuantity: raw.inventoryQuantity ?? null,
    };
}

export function productFrom(raw: RawProduct): Product {
    const summary = productSummaryFrom(raw);
    const variants: Variant[] = [];
    for (const variant of raw.variants?.nodes ?? []) variants.push(variantFrom(variant));
    return {
        id: summary.id,
        title: summary.title,
        handle: summary.handle,
        status: summary.status,
        vendor: summary.vendor,
        productType: summary.productType,
        tags: summary.tags,
        totalInventory: summary.totalInventory,
        minPrice: summary.minPrice,
        currency: summary.currency,
        descriptionHtml: text(raw.descriptionHtml),
        variants: variants,
        moreVariants: raw.variants?.pageInfo?.hasNextPage === true,
    };
}

export function locationFrom(raw: RawLocation): Location {
    return { id: text(raw.id), name: text(raw.name), isActive: raw.isActive ?? false, fulfillsOnlineOrders: raw.fulfillsOnlineOrders ?? false };
}

function quantityNamed(quantities: RawQuantity[] | null | undefined, name: string): number | null {
    for (const quantity of quantities ?? []) {
        if (quantity.name === name) return quantity.quantity ?? null;
    }
    return null;
}

export function variantInventoryFrom(raw: RawInventoryVariant): VariantInventory {
    const levels: InventoryLevel[] = [];
    for (const level of raw.inventoryItem?.inventoryLevels?.nodes ?? []) {
        levels.push({
            locationId: text(level.location?.id),
            locationName: text(level.location?.name),
            available: quantityNamed(level.quantities, "available"),
            committed: quantityNamed(level.quantities, "committed"),
            onHand: quantityNamed(level.quantities, "on_hand"),
        });
    }
    return { variantId: text(raw.id), title: text(raw.title), inventoryItemId: text(raw.inventoryItem?.id), levels: levels };
}

// The quantity a set compares against. Missing means the item is not stocked there.
export function availableFrom(raw: RawInventoryItem | null | undefined): number {
    const available = quantityNamed(raw?.inventoryLevel?.quantities, "available");
    if (available === null) throw validationError("not_stocked", "this inventory item is not stocked at this location");
    return available;
}

export function customerSummaryFrom(raw: RawCustomer): CustomerSummary {
    return {
        id: text(raw.id),
        firstName: raw.firstName ?? null,
        lastName: raw.lastName ?? null,
        email: raw.defaultEmailAddress?.emailAddress ?? null,
        phone: raw.defaultPhoneNumber?.phoneNumber ?? null,
        numberOfOrders: Number(raw.numberOfOrders ?? "0"),
    };
}

export function customerFrom(raw: RawCustomer): Customer {
    const summary = customerSummaryFrom(raw);
    const orders: OrderSummary[] = [];
    for (const order of raw.orders?.nodes ?? []) orders.push(orderSummaryFrom(order));
    return {
        id: summary.id,
        firstName: summary.firstName,
        lastName: summary.lastName,
        email: summary.email,
        phone: summary.phone,
        numberOfOrders: summary.numberOfOrders,
        amountSpent: decimalOrNull(raw.amountSpent?.amount),
        currency: raw.amountSpent?.currencyCode ?? null,
        orders: orders,
    };
}

export function abandonedFrom(raw: RawAbandonedCheckout): AbandonedCheckout {
    return {
        id: text(raw.id),
        name: text(raw.name),
        createdAt: text(raw.createdAt),
        completedAt: raw.completedAt ?? null,
        url: text(raw.abandonedCheckoutUrl),
        email: raw.customer?.defaultEmailAddress?.emailAddress ?? null,
        total: moneyAmount(raw.totalPriceSet),
        currency: moneyCurrency(raw.totalPriceSet),
    };
}

export function discountFrom(raw: RawDiscountResult | null | undefined): DiscountCode {
    requireNoUserErrors(raw?.userErrors);
    return { id: requireId(raw?.codeDiscountNode?.id, "discount") };
}

export function draftOrderFrom(raw: RawDraftResult | null | undefined): DraftOrder {
    requireNoUserErrors(raw?.userErrors);
    const draft = raw?.draftOrder;
    if (draft === null || draft === undefined) throw validationError("unexpected_response", "Shopify sent no draft order");
    return {
        id: text(draft.id),
        name: text(draft.name),
        status: text(draft.status),
        invoiceUrl: draft.invoiceUrl ?? null,
        total: moneyAmount(draft.totalPriceSet),
        currency: moneyCurrency(draft.totalPriceSet),
    };
}

export function shopFrom(raw: RawShop | null | undefined): Shop {
    if (raw === null || raw === undefined) throw validationError("unexpected_response", "Shopify sent no shop");
    return {
        name: text(raw.name),
        email: text(raw.email),
        currency: text(raw.currencyCode),
        timezone: text(raw.ianaTimezone),
        domain: text(raw.myshopifyDomain),
        plan: text(raw.plan?.publicDisplayName),
    };
}

// A write's `userErrors` mean Shopify refused it. Applying nothing, it answers 200.
export function requireNoUserErrors(errors: RawUserError[] | null | undefined): void {
    for (const error of errors ?? []) {
        const message = text(error.message);
        const field = (error.field ?? []).join(".");
        throw new ShopifyError(
            text(error.code).length > 0 ? text(error.code) : "user_error",
            "Shopify refused this: " + message + (field.length > 0 ? " (" + field + ")" : ""),
            200,
            false,
            null,
            field.length > 0 ? field : null,
        );
    }
}

// Seconds until the bucket refills enough for this query, or null when unknown.
export function throttleWait(cost: RawCost | null | undefined): number | null {
    const requested = cost?.requestedQueryCost;
    const available = cost?.throttleStatus?.currentlyAvailable;
    const rate = cost?.throttleStatus?.restoreRate;
    if (requested === null || requested === undefined || available === null || available === undefined || rate === null || rate === undefined || rate <= 0) {
        return null;
    }
    return Math.max(1, Math.ceil((requested - available) / rate));
}

// The first GraphQL error as a ShopifyError, or null when the answer has none.
export function graphQlFailure(status: GraphQlStatus): ShopifyError | null {
    for (const error of status.errors ?? []) {
        const code = text(error.extensions?.code);
        const throttled = code === "THROTTLED";
        return new ShopifyError(
            throttled ? "throttled" : code.length > 0 ? code : "graphql_error",
            "Shopify GraphQL error: " + text(error.message),
            200,
            throttled,
            throttled ? throttleWait(status.extensions?.cost) : null,
            null,
        );
    }
    return null;
}

function errorCodeFor(status: number): string {
    if (status === 401) return "unauthorized";
    if (status === 402) return "payment_required";
    if (status === 403) return "forbidden";
    if (status === 404) return "not_found";
    if (status === 423) return "shop_locked";
    if (status === 429) return "throttled";
    if (status >= 500) return "server_error";
    return "http_" + status.toString();
}

// Shopify's HTTP errors hold a string in `errors`, unlike its GraphQL errors.
function httpErrorDetail(body: string): string {
    try {
        return (JSON.parse(body) as HttpErrorBody).errors ?? "";
    } catch (error) {
        return "";
    }
}

export function httpFailure(status: number, statusText: string, body: string, retryAfter: number | null): ShopifyError {
    const detail = httpErrorDetail(body);
    const message = detail.length > 0
        ? "Shopify " + status.toString() + ": " + detail
        : "Shopify request failed: HTTP " + status.toString() + " " + statusText;
    return new ShopifyError(errorCodeFor(status), message, status, status === 429, retryAfter, null);
}

// Seconds from a `Retry-After` header, which Shopify sends with a decimal point, or null.
export function retryAfterFrom(header: string): number | null {
    const value = header.trim();
    if (value.length === 0) return null;
    const seconds = Number(value);
    return Number.isFinite(seconds) && seconds >= 0 ? Math.ceil(seconds) : null;
}
