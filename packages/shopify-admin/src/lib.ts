// A synchronous, typed client for the Shopify GraphQL Admin API, version 2026-10.
// The access token and the store are read from the SHOPIFY_ACCESS_TOKEN and
// SHOPIFY_SHOP secrets inside the package; neither is an argument and the token
// is never returned. Every operation normalizes its input, resolves what a policy
// needs from Shopify (an order's total, a refund's amount, which order a record
// belongs to), checks those values, and sends exactly those values.
//
// `shop` is in every check, so a policy can pin one store. The host holds the
// store's name, so `build check` cannot derive a host filter for `http.post`, and
// warns once at the one call in this file. A blueprint supplies that filter.
//
// There are no delete operations, and a product or a customer cannot be created.

import { post, Response } from "submilli:http";
import secrets from "submilli:secrets";
import { check } from "submilli:security";
import { ShopifyError, validationError } from "./errors";
import {
    AVAILABLE_AT,
    DISCOUNT_CREATE,
    DISCOUNT_DEACTIVATE,
    DRAFT_ORDER_CREATE,
    DRAFT_ORDER_SEND_INVOICE,
    FULFILLMENT_CREATE,
    FULFILLMENT_ORDER_HOLD,
    FULFILLMENT_ORDER_OF,
    FULFILLMENT_ORDER_RELEASE,
    FULFILLMENT_TRACKING_UPDATE,
    GET_CUSTOMER,
    GET_ORDER,
    GET_PRODUCT,
    GET_SHOP,
    INVENTORY_ADJUST,
    INVENTORY_LEVELS,
    INVENTORY_SET,
    LIST_ABANDONED_CHECKOUTS,
    LIST_FULFILLMENT_ORDERS,
    LIST_LOCATIONS,
    OPEN_FULFILLMENT_ORDERS,
    ORDER_CANCEL,
    ORDER_FACTS,
    ORDER_UPDATE,
    PRODUCT_UPDATE,
    REFUND_CREATE,
    SEARCH_CUSTOMERS,
    SEARCH_ORDERS,
    SEARCH_PRODUCTS,
    SUGGEST_REFUND,
    VARIANT_PRICES_UPDATE,
} from "./queries";
import {
    buildCancelVariables,
    buildDiscount,
    buildDraftOrder,
    buildFulfillment,
    buildInventoryAdjust,
    buildInventorySet,
    buildOrderSearchVariables,
    buildRefundLines,
    buildSearchVariables,
    buildTracking,
    buildVariantPrices,
    CancelInput,
    DiscountInput,
    DraftOrderInput,
    FulfillmentInput,
    inventoryWindow,
    normalizeAmount,
    normalizeCancelReason,
    normalizeCode,
    normalizeDelta,
    normalizeEmail,
    normalizeGid,
    normalizeHoldReason,
    normalizeHttpsUrl,
    normalizeInstant,
    normalizeKey,
    normalizeNote,
    normalizePercentage,
    normalizeProductStatus,
    normalizeQuantity,
    normalizeRestockType,
    normalizeShop,
    normalizeSortKey,
    normalizeStaffNote,
    normalizeStockLevel,
    normalizeTag,
    normalizeUsageLimit,
    OrderSearchInput,
    PageOptions,
    pageWindow,
    PreparedDraftLine,
    PreparedRefundLine,
    PreparedVariantPrice,
    prepareDraftLine,
    prepareRefundLine,
    prepareVariantPrice,
    productFields,
    ProductUpdate,
    RefundInput,
    RefundQuery,
    requireLineCount,
    requireOneDiscountValue,
    requireQuery,
    requireTagCount,
    requireText,
    SearchInput,
    TrackingInput,
    VariantPriceInput,
} from "./request";
import {
    AbandonedCheckout,
    AbandonedData,
    abandonedFrom,
    appendNote,
    AvailableData,
    availableFrom,
    CancelJob,
    cancelJobFrom,
    chooseFulfillmentOrders,
    Customer,
    CustomerData,
    customerFrom,
    CustomersData,
    CustomerSummary,
    customerSummaryFrom,
    DiscountCode,
    DiscountCreateData,
    DiscountDeactivateData,
    discountFrom,
    DraftCreateData,
    DraftOrder,
    draftOrderFrom,
    DraftSendData,
    Fulfillment,
    FulfillmentCreateData,
    fulfillmentFrom,
    FulfillmentOrder,
    fulfillmentOrderFrom,
    FulfillmentOrdersData,
    FulfillmentOrderState,
    fulfillmentOrderStateFrom,
    FulfillmentOwnerData,
    FulfillmentTrackingData,
    GraphQlResponse,
    GraphQlStatus,
    graphQlFailure,
    HoldData,
    httpFailure,
    InventoryAdjustData,
    InventoryLevelsData,
    InventorySetData,
    Location,
    locationFrom,
    LocationsData,
    mergeTags,
    nextCursorFrom,
    Order,
    OrderCancelData,
    OrderData,
    orderFrom,
    OrdersData,
    OrderSummary,
    orderSummaryFrom,
    OrderUpdateData,
    Page,
    Product,
    ProductData,
    productFrom,
    ProductsData,
    ProductSummary,
    productSummaryFrom,
    ProductUpdateData,
    RawInventoryWrite,
    RawOrder,
    Refund,
    RefundCreateData,
    refundFrom,
    ReleaseData,
    requireAmount,
    requireId,
    requireNoUserErrors,
    retryAfterFrom,
    Shop,
    ShopData,
    shopFrom,
    SuggestedRefund,
    suggestedRefundFrom,
    SuggestionData,
    Variant,
    variantFrom,
    VariantInventory,
    variantInventoryFrom,
    VariantsUpdateData,
} from "./response";

export { ShopifyError } from "./errors";
export {
    CancelInput,
    DiscountInput,
    DraftDiscountInput,
    DraftLineInput,
    DraftOrderInput,
    FulfillmentInput,
    OrderSearchInput,
    PageOptions,
    ProductUpdate,
    RefundInput,
    RefundLineInput,
    RefundQuery,
    SearchInput,
    TrackingInput,
    VariantPriceInput,
} from "./request";
export {
    AbandonedCheckout,
    CancelJob,
    Customer,
    CustomerSummary,
    DiscountCode,
    DraftOrder,
    Fulfillment,
    FulfillmentHold,
    FulfillmentOrder,
    FulfillmentOrderLine,
    FulfillmentOrderState,
    InventoryLevel,
    Location,
    Order,
    OrderLine,
    OrderSummary,
    Page,
    Product,
    ProductSummary,
    Refund,
    Shop,
    SuggestedRefund,
    SuggestedTransaction,
    Tracking,
    Variant,
    VariantInventory,
} from "./response";

const GRAPHQL_PATH = ".myshopify.com/admin/api/2026-10/graphql.json";
const NO_RESTOCK = "NO_RESTOCK";

/**
 * Search the store's orders, newest first unless `sortKey` or `reverse` says otherwise.
 * @param input The search, page size and cursor.
 * @returns One page of orders.
 * @capability shopify.com/orders.search { shop: string }
 */
export function searchOrders(input: OrderSearchInput = {}): Page<OrderSummary> {
    const { query, first, after, sortKey, reverse } = input;
    const search = requireQuery(query);
    const window = pageWindow(first, after);
    const sort = normalizeSortKey(sortKey);
    const shop = requireShop();
    check("shopify.com/orders.search", { shop: shop });
    const variables = buildOrderSearchVariables(window, search, sort, reverse ?? true);
    const orders = (graphql(shop, SEARCH_ORDERS, variables).json() as GraphQlResponse<OrdersData>).data?.orders;
    const items: OrderSummary[] = [];
    for (const raw of orders?.nodes ?? []) items.push(orderSummaryFrom(raw));
    return { items: items, nextCursor: nextCursorFrom(orders?.pageInfo) };
}

/**
 * Get one order, with its first 100 lines; null when it does not exist.
 * @param orderId Order id: a number or a `gid://shopify/Order/` id.
 * @returns The order, or null.
 * @capability shopify.com/orders.get { shop: string, orderId: string }
 */
export function getOrder(orderId: string): Order | null {
    const id = normalizeGid(orderId, "Order", "orderId");
    const shop = requireShop();
    check("shopify.com/orders.get", { shop: shop, orderId: id });
    const raw = (graphql(shop, GET_ORDER, { id: id }).json() as GraphQlResponse<OrderData>).data?.order;
    return raw === null || raw === undefined ? null : orderFrom(raw);
}

/**
 * Add a line to an order's note for staff. What is already there stays.
 * @param orderId Order id.
 * @param note The line to add, up to 5000 characters.
 * @returns The order's note now.
 * @capability shopify.com/orders.update { shop: string, orderId: string, fields: string[] }
 */
export function addOrderNote(orderId: string, note: string): string {
    const id = normalizeGid(orderId, "Order", "orderId");
    const line = normalizeNote(note);
    const shop = requireShop();
    check("shopify.com/orders.update", { shop: shop, orderId: id, fields: ["note"] });
    const order = requireOrderFacts(shop, id);
    const merged = appendNote(order.note, line);
    const result = (graphql(shop, ORDER_UPDATE, { input: { id: id, note: merged } }).json() as GraphQlResponse<OrderUpdateData>).data?.orderUpdate;
    requireNoUserErrors(result?.userErrors);
    return result?.order?.note ?? merged;
}

/**
 * Add tags to an order. Tags already there stay.
 * @param orderId Order id.
 * @param tags The tags to add, up to 40 characters each, without commas.
 * @returns The order's tags now.
 * @capability shopify.com/orders.update { shop: string, orderId: string, fields: string[] }
 */
export function addOrderTags(orderId: string, tags: string[]): string[] {
    const id = normalizeGid(orderId, "Order", "orderId");
    const added: string[] = [];
    for (const tag of tags) added.push(normalizeTag(tag));
    requireTagCount(added.length);
    const shop = requireShop();
    check("shopify.com/orders.update", { shop: shop, orderId: id, fields: ["tags"] });
    const merged = mergeTags(requireOrderFacts(shop, id).tags ?? [], added);
    requireTagCount(merged.length);
    const result = (graphql(shop, ORDER_UPDATE, { input: { id: id, tags: merged } }).json() as GraphQlResponse<OrderUpdateData>).data?.orderUpdate;
    requireNoUserErrors(result?.userErrors);
    return result?.order?.tags ?? merged;
}

/**
 * Cancel an order. This cannot be undone. The order is fetched first, so a policy sees its real total
 * and currency. Shopify finishes the cancellation in the background.
 * @param orderId Order id.
 * @param input The reason, and whether to restock, refund and tell the customer.
 * @returns The job that finishes the cancellation.
 * @capability shopify.com/orders.cancel { shop: string, orderId: string, totalAmount: number, currency: string, restock: boolean, refund: boolean }
 */
export function cancelOrder(orderId: string, input: CancelInput): CancelJob {
    const { reason, restock, refund, notifyCustomer, staffNote } = input;
    const id = normalizeGid(orderId, "Order", "orderId");
    const why = normalizeCancelReason(reason);
    const shouldRestock = restock === true;
    const shouldRefund = refund === true;
    const notify = notifyCustomer === true;
    const memo = staffNote === undefined ? null : normalizeStaffNote(staffNote);
    const shop = requireShop();
    const order = requireOrderFacts(shop, id);
    if (order.cancelledAt !== null && order.cancelledAt !== undefined) {
        throw validationError("already_cancelled", "order " + id + " is already canceled");
    }
    const money = order.currentTotalPriceSet?.presentmentMoney;
    const totalAmount = requireAmount(money?.amount, "order total");
    const currency = money?.currencyCode ?? "";
    if (currency.length === 0) throw validationError("unexpected_response", "Shopify sent no order currency");
    check("shopify.com/orders.cancel", {
        shop: shop,
        orderId: id,
        totalAmount: totalAmount,
        currency: currency,
        restock: shouldRestock,
        refund: shouldRefund,
    });
    const variables = buildCancelVariables(id, why, shouldRestock, shouldRefund, notify, memo);
    return cancelJobFrom((graphql(shop, ORDER_CANCEL, variables).json() as GraphQlResponse<OrderCancelData>).data?.orderCancel);
}

/**
 * Work out what a refund would return, without refunding. Null when the order does not exist.
 * @param orderId Order id.
 * @param input The lines, and whether to include shipping.
 * @returns The suggested refund, or null.
 * @capability shopify.com/refunds.suggest { shop: string, orderId: string }
 */
export function suggestRefund(orderId: string, input: RefundQuery): SuggestedRefund | null {
    const { lineItems, shipping } = input;
    const id = normalizeGid(orderId, "Order", "orderId");
    const lines: PreparedRefundLine[] = [];
    for (const line of lineItems ?? []) {
        const { lineItemId, quantity } = line;
        lines.push(prepareRefundLine(lineItemId, quantity));
    }
    const refundShipping = shipping === true;
    if (lines.length === 0 && !refundShipping) throw validationError("nothing_to_refund", "name lineItems, or set shipping");
    const shop = requireShop();
    check("shopify.com/refunds.suggest", { shop: shop, orderId: id });
    return fetchSuggestion(shop, id, buildRefundLines(lines, NO_RESTOCK, null), refundShipping);
}

/**
 * Refund lines of an order, its shipping, or both. Shopify works out the amount and the payment
 * transactions first, so a policy sees the real amount and currency.
 * @param orderId Order id.
 * @param input What to refund, and a new `idempotencyKey`.
 * @returns The refund.
 * @capability shopify.com/refunds.create { shop: string, orderId: string, amount: number, currency: string, restock: boolean }
 */
export function refundOrder(orderId: string, input: RefundInput): Refund {
    const { lineItems, shipping, restockType, locationId, note, notify, idempotencyKey } = input;
    const id = normalizeGid(orderId, "Order", "orderId");
    const key = normalizeKey(idempotencyKey);
    const restock = normalizeRestockType(restockType);
    const location = locationId === undefined ? null : normalizeGid(locationId, "Location", "locationId");
    const lines: PreparedRefundLine[] = [];
    for (const line of lineItems ?? []) {
        const { lineItemId, quantity } = line;
        lines.push(prepareRefundLine(lineItemId, quantity));
    }
    const refundShipping = shipping === true;
    if (lines.length === 0 && !refundShipping) throw validationError("nothing_to_refund", "name lineItems, or set shipping");
    const restocks = restock !== NO_RESTOCK;
    if (restocks && lines.length === 0) throw validationError("restock_needs_line_items", "name the lineItems to restock");
    if (restocks && location === null) throw validationError("restock_needs_location", "name the locationId to restock at");
    const memo = note === undefined ? null : normalizeNote(note);
    const tellCustomer = notify === true;
    const shop = requireShop();
    const refundLines = buildRefundLines(lines, restock, location);
    const suggestion = fetchSuggestion(shop, id, refundLines, refundShipping);
    if (suggestion === null) throw validationError("not_found", "no order " + id);
    if (suggestion.amount <= 0) throw validationError("nothing_to_refund", "Shopify suggests no money to return for this");
    if (suggestion.currency.length === 0) throw validationError("unexpected_response", "Shopify sent no refund currency");
    check("shopify.com/refunds.create", {
        shop: shop,
        orderId: id,
        amount: suggestion.amount,
        currency: suggestion.currency,
        restock: restocks,
    });
    const transactions: Record<string, unknown>[] = [];
    for (const transaction of suggestion.transactions) {
        const item: Record<string, unknown> = { orderId: id, kind: "REFUND", gateway: transaction.gateway, amount: transaction.amountText };
        if (transaction.parentTransactionId !== null) item["parentId"] = transaction.parentTransactionId;
        transactions.push(item);
    }
    const refund: Record<string, unknown> = { orderId: id, currency: suggestion.currency, notify: tellCustomer, transactions: transactions };
    if (refundLines.length > 0) refund["refundLineItems"] = refundLines;
    if (refundShipping) refund["shipping"] = { fullRefund: true };
    if (memo !== null) refund["note"] = memo;
    return refundFrom((graphql(shop, REFUND_CREATE, { input: refund, idempotencyKey: key }).json() as GraphQlResponse<RefundCreateData>).data?.refundCreate);
}

/**
 * List an order's fulfillment orders; null when the order does not exist.
 * @param orderId Order id.
 * @returns The first 25, each with its first 20 lines, or null.
 * @capability shopify.com/fulfillmentOrders.list { shop: string, orderId: string }
 */
export function listFulfillmentOrders(orderId: string): FulfillmentOrder[] | null {
    const id = normalizeGid(orderId, "Order", "orderId");
    const shop = requireShop();
    check("shopify.com/fulfillmentOrders.list", { shop: shop, orderId: id });
    const order = (graphql(shop, LIST_FULFILLMENT_ORDERS, { id: id }).json() as GraphQlResponse<FulfillmentOrdersData>).data?.order;
    if (order === null || order === undefined) return null;
    const items: FulfillmentOrder[] = [];
    for (const raw of order.fulfillmentOrders?.nodes ?? []) items.push(fulfillmentOrderFrom(raw));
    return items;
}

/**
 * Record a shipment of an order. Each fulfillment order named must be open on this order, and all must
 * be at one location. With none named, every open one is fulfilled.
 * @param orderId Order id.
 * @param input What to fulfill, the tracking, and whether to tell the customer.
 * @returns The fulfillment.
 * @capability shopify.com/fulfillments.create { shop: string, orderId: string, notifyCustomer: boolean }
 */
export function createFulfillment(orderId: string, input: FulfillmentInput = {}): Fulfillment {
    const { fulfillmentOrderIds, tracking, notifyCustomer } = input;
    const id = normalizeGid(orderId, "Order", "orderId");
    const chosen: string[] = [];
    for (const value of fulfillmentOrderIds ?? []) chosen.push(normalizeGid(value, "FulfillmentOrder", "fulfillmentOrderId"));
    if (fulfillmentOrderIds !== undefined && chosen.length === 0) throw validationError("invalid_line_items", "name at least one fulfillment order, or omit fulfillmentOrderIds");
    const trackingInfo = tracking === undefined ? null : trackingFrom(tracking);
    const notify = notifyCustomer === true;
    const shop = requireShop();
    const order = (graphql(shop, OPEN_FULFILLMENT_ORDERS, { id: id }).json() as GraphQlResponse<FulfillmentOrdersData>).data?.order;
    if (order === null || order === undefined) throw validationError("not_found", "no order " + id);
    const targets = chooseFulfillmentOrders(order.fulfillmentOrders?.nodes ?? [], chosen);
    check("shopify.com/fulfillments.create", { shop: shop, orderId: id, notifyCustomer: notify });
    const variables = { fulfillment: buildFulfillment(targets, trackingInfo, notify) };
    return fulfillmentFrom((graphql(shop, FULFILLMENT_CREATE, variables).json() as GraphQlResponse<FulfillmentCreateData>).data?.fulfillmentCreate);
}

/**
 * Change the tracking on a fulfillment. Its order is resolved from Shopify, so a policy sees the real one.
 * @param fulfillmentId Fulfillment id.
 * @param tracking The new tracking details.
 * @param notifyCustomer Tell the customer. Default false.
 * @returns The fulfillment.
 * @capability shopify.com/fulfillments.updateTracking { shop: string, orderId: string, fulfillmentId: string }
 */
export function updateTracking(fulfillmentId: string, tracking: TrackingInput, notifyCustomer: boolean = false): Fulfillment {
    const id = normalizeGid(fulfillmentId, "Fulfillment", "fulfillmentId");
    const trackingInfo = trackingFrom(tracking);
    const shop = requireShop();
    const fulfillment = (graphql(shop, FULFILLMENT_ORDER_OF, { id: id }).json() as GraphQlResponse<FulfillmentOwnerData>).data?.fulfillment;
    if (fulfillment === null || fulfillment === undefined) throw validationError("not_found", "no fulfillment " + id);
    const orderId = requireId(fulfillment.order?.id, "order for this fulfillment");
    check("shopify.com/fulfillments.updateTracking", { shop: shop, orderId: orderId, fulfillmentId: id });
    const variables = { fulfillmentId: id, trackingInfoInput: trackingInfo, notifyCustomer: notifyCustomer === true };
    return fulfillmentFrom((graphql(shop, FULFILLMENT_TRACKING_UPDATE, variables).json() as GraphQlResponse<FulfillmentTrackingData>).data?.fulfillmentTrackingInfoUpdate);
}

/**
 * Put a fulfillment order on hold, so it does not ship.
 * @param fulfillmentOrderId Fulfillment order id.
 * @param reason `AWAITING_PAYMENT`, `HIGH_RISK_OF_FRAUD`, `INCORRECT_ADDRESS`, `INVENTORY_OUT_OF_STOCK`, `UNKNOWN_DELIVERY_DATE`, `AWAITING_RETURN_ITEMS` or `OTHER`.
 * @param notes A note for staff.
 * @returns The fulfillment order, with the new hold's id.
 * @capability shopify.com/fulfillmentOrders.hold { shop: string, fulfillmentOrderId: string, reason: string }
 */
export function holdFulfillmentOrder(fulfillmentOrderId: string, reason: string, notes?: string): FulfillmentOrderState {
    const id = normalizeGid(fulfillmentOrderId, "FulfillmentOrder", "fulfillmentOrderId");
    const why = normalizeHoldReason(reason);
    const memo = notes === undefined ? null : normalizeStaffNote(notes);
    const shop = requireShop();
    check("shopify.com/fulfillmentOrders.hold", { shop: shop, fulfillmentOrderId: id, reason: why });
    const hold: Record<string, unknown> = { reason: why };
    if (memo !== null) hold["reasonNotes"] = memo;
    return fulfillmentOrderStateFrom((graphql(shop, FULFILLMENT_ORDER_HOLD, { id: id, fulfillmentHold: hold }).json() as GraphQlResponse<HoldData>).data?.fulfillmentOrderHold);
}

/**
 * Release holds on a fulfillment order.
 * @param fulfillmentOrderId Fulfillment order id.
 * @param holdIds The holds to release. Omit it to release every hold.
 * @returns The fulfillment order.
 * @capability shopify.com/fulfillmentOrders.release { shop: string, fulfillmentOrderId: string }
 */
export function releaseFulfillmentOrderHold(fulfillmentOrderId: string, holdIds?: string[]): FulfillmentOrderState {
    const id = normalizeGid(fulfillmentOrderId, "FulfillmentOrder", "fulfillmentOrderId");
    const holds: string[] = [];
    for (const hold of holdIds ?? []) holds.push(normalizeGid(hold, "FulfillmentHold", "holdId"));
    const shop = requireShop();
    check("shopify.com/fulfillmentOrders.release", { shop: shop, fulfillmentOrderId: id });
    const variables: Record<string, unknown> = { id: id };
    if (holds.length > 0) variables["holdIds"] = holds;
    return fulfillmentOrderStateFrom((graphql(shop, FULFILLMENT_ORDER_RELEASE, variables).json() as GraphQlResponse<ReleaseData>).data?.fulfillmentOrderReleaseHold);
}

/**
 * Search the catalog.
 * @param input The search, page size and cursor.
 * @returns One page of products.
 * @capability shopify.com/products.search { shop: string }
 */
export function searchProducts(input: SearchInput = {}): Page<ProductSummary> {
    const { query, first, after } = input;
    const search = requireQuery(query);
    const window = pageWindow(first, after);
    const shop = requireShop();
    check("shopify.com/products.search", { shop: shop });
    const products = (graphql(shop, SEARCH_PRODUCTS, buildSearchVariables(window, search)).json() as GraphQlResponse<ProductsData>).data?.products;
    const items: ProductSummary[] = [];
    for (const raw of products?.nodes ?? []) items.push(productSummaryFrom(raw));
    return { items: items, nextCursor: nextCursorFrom(products?.pageInfo) };
}

/**
 * Get one product, with its first 100 variants; null when it does not exist.
 * @param productId Product id.
 * @returns The product, or null.
 * @capability shopify.com/products.get { shop: string, productId: string }
 */
export function getProduct(productId: string): Product | null {
    const id = normalizeGid(productId, "Product", "productId");
    const shop = requireShop();
    check("shopify.com/products.get", { shop: shop, productId: id });
    const raw = (graphql(shop, GET_PRODUCT, { id: id }).json() as GraphQlResponse<ProductData>).data?.product;
    return raw === null || raw === undefined ? null : productFrom(raw);
}

/**
 * Change a product. The check lists the field names set, so a policy can forbid `status`.
 * @param productId Product id.
 * @param fields The fields to change.
 * @returns The product.
 * @capability shopify.com/products.update { shop: string, productId: string, fields: string[] }
 */
export function updateProduct(productId: string, fields: ProductUpdate): ProductSummary {
    const { title, descriptionHtml, status, tags, vendor, productType } = fields;
    const id = normalizeGid(productId, "Product", "productId");
    const newTitle = title === undefined ? null : requireText(title, "title", 255);
    const newDescription = descriptionHtml === undefined ? null : requireText(descriptionHtml, "descriptionHtml", 100000);
    const newStatus = status === undefined ? null : normalizeProductStatus(status);
    const newVendor = vendor === undefined ? null : requireText(vendor, "vendor", 255);
    const newType = productType === undefined ? null : requireText(productType, "productType", 255);
    const newTags: string[] = [];
    for (const tag of tags ?? []) newTags.push(normalizeTag(tag));
    const setsTags = tags !== undefined;
    const names = productFields(newTitle !== null, newDescription !== null, newStatus !== null, setsTags, newVendor !== null, newType !== null);
    const shop = requireShop();
    check("shopify.com/products.update", { shop: shop, productId: id, fields: names });
    const product: Record<string, unknown> = { id: id };
    if (newTitle !== null) product["title"] = newTitle;
    if (newDescription !== null) product["descriptionHtml"] = newDescription;
    if (newStatus !== null) product["status"] = newStatus;
    if (setsTags) product["tags"] = newTags;
    if (newVendor !== null) product["vendor"] = newVendor;
    if (newType !== null) product["productType"] = newType;
    const result = (graphql(shop, PRODUCT_UPDATE, { product: product }).json() as GraphQlResponse<ProductUpdateData>).data?.productUpdate;
    requireNoUserErrors(result?.userErrors);
    const raw = result?.product;
    if (raw === null || raw === undefined) throw validationError("unexpected_response", "Shopify sent no product");
    return productSummaryFrom(raw);
}

/**
 * Change prices of a product's variants. Each variant must belong to the product.
 * @param productId Product id.
 * @param prices New prices, 1-100 variants.
 * @returns The variants.
 * @capability shopify.com/variants.updatePrices { shop: string, productId: string, variantIds: string[], maxPrice: number }
 */
export function updateVariantPrices(productId: string, prices: VariantPriceInput[]): Variant[] {
    const id = normalizeGid(productId, "Product", "productId");
    const prepared: PreparedVariantPrice[] = [];
    const variantIds: string[] = [];
    let maxPrice = 0;
    for (const item of prices) {
        const { variantId, price, compareAtPrice } = item;
        const line = prepareVariantPrice(variantId, price, compareAtPrice);
        prepared.push(line);
        variantIds.push(line.variantId);
        maxPrice = Math.max(maxPrice, line.price);
    }
    requireLineCount(prepared.length);
    const shop = requireShop();
    check("shopify.com/variants.updatePrices", { shop: shop, productId: id, variantIds: variantIds, maxPrice: maxPrice });
    const variables = { productId: id, variants: buildVariantPrices(prepared) };
    const result = (graphql(shop, VARIANT_PRICES_UPDATE, variables).json() as GraphQlResponse<VariantsUpdateData>).data?.productVariantsBulkUpdate;
    requireNoUserErrors(result?.userErrors);
    const items: Variant[] = [];
    for (const raw of result?.productVariants ?? []) items.push(variantFrom(raw));
    return items;
}

/**
 * List the store's first 50 locations.
 * @returns The locations.
 * @capability shopify.com/locations.list { shop: string }
 */
export function listLocations(): Location[] {
    const shop = requireShop();
    check("shopify.com/locations.list", { shop: shop });
    const items: Location[] = [];
    for (const raw of (graphql(shop, LIST_LOCATIONS, {}).json() as GraphQlResponse<LocationsData>).data?.locations?.nodes ?? []) items.push(locationFrom(raw));
    return items;
}

/**
 * Get a product's stock, by variant and location; null when the product does not exist. Also the way to
 * find inventory item ids.
 * @param productId Product id.
 * @param page Page size, 1-15 (default 15), and cursor. Shopify's query cost limit keeps this page small.
 * @returns One page of variants, or null.
 * @capability shopify.com/inventory.get { shop: string, productId: string }
 */
export function getInventoryLevels(productId: string, page: PageOptions = {}): Page<VariantInventory> | null {
    const { first, after } = page;
    const id = normalizeGid(productId, "Product", "productId");
    const window = inventoryWindow(first, after);
    const shop = requireShop();
    check("shopify.com/inventory.get", { shop: shop, productId: id });
    const variables: Record<string, unknown> = { id: id, first: window.first };
    if (window.after !== null) variables["after"] = window.after;
    const product = (graphql(shop, INVENTORY_LEVELS, variables).json() as GraphQlResponse<InventoryLevelsData>).data?.product;
    if (product === null || product === undefined) return null;
    const items: VariantInventory[] = [];
    for (const raw of product.variants?.nodes ?? []) items.push(variantInventoryFrom(raw));
    return { items: items, nextCursor: nextCursorFrom(product.variants?.pageInfo) };
}

/**
 * Set the quantity available at a location. The current level is read first, so a policy sees the real
 * change, and Shopify refuses the write with `CHANGE_FROM_QUANTITY_STALE` if stock moved in between.
 * @param inventoryItemId Inventory item id, from `getInventoryLevels`.
 * @param locationId Location id.
 * @param quantity New quantity available.
 * @param reason Shopify's reason, such as `correction`, `cycle_count_available` or `received`.
 * @param idempotencyKey A new unique string for each change, such as a UUID.
 * @capability shopify.com/inventory.set { shop: string, inventoryItemId: string, locationId: string, quantity: number, delta: number }
 */
export function setInventory(inventoryItemId: string, locationId: string, quantity: number, reason: string, idempotencyKey: string): void {
    const item = normalizeGid(inventoryItemId, "InventoryItem", "inventoryItemId");
    const location = normalizeGid(locationId, "Location", "locationId");
    const level = normalizeStockLevel(quantity);
    const why = requireText(reason, "reason", 64);
    const key = normalizeKey(idempotencyKey);
    const shop = requireShop();
    const current = availableFrom((graphql(shop, AVAILABLE_AT, { id: item, locationId: location }).json() as GraphQlResponse<AvailableData>).data?.inventoryItem);
    check("shopify.com/inventory.set", { shop: shop, inventoryItemId: item, locationId: location, quantity: level, delta: level - current });
    const variables = { input: buildInventorySet(item, location, level, current, why), idempotencyKey: key };
    inventoryWritten((graphql(shop, INVENTORY_SET, variables).json() as GraphQlResponse<InventorySetData>).data?.inventorySetQuantities);
}

/**
 * Add to or take from the quantity available at a location.
 * @param inventoryItemId Inventory item id, from `getInventoryLevels`.
 * @param locationId Location id.
 * @param delta Change: positive adds, negative removes; not zero.
 * @param reason Shopify's reason, such as `correction`, `damaged` or `received`.
 * @param idempotencyKey A new unique string for each change, such as a UUID.
 * @capability shopify.com/inventory.adjust { shop: string, inventoryItemId: string, locationId: string, delta: number }
 */
export function adjustInventory(inventoryItemId: string, locationId: string, delta: number, reason: string, idempotencyKey: string): void {
    const item = normalizeGid(inventoryItemId, "InventoryItem", "inventoryItemId");
    const location = normalizeGid(locationId, "Location", "locationId");
    const change = normalizeDelta(delta);
    const why = requireText(reason, "reason", 64);
    const key = normalizeKey(idempotencyKey);
    const shop = requireShop();
    check("shopify.com/inventory.adjust", { shop: shop, inventoryItemId: item, locationId: location, delta: change });
    const variables = { input: buildInventoryAdjust(item, location, change, why), idempotencyKey: key };
    inventoryWritten((graphql(shop, INVENTORY_ADJUST, variables).json() as GraphQlResponse<InventoryAdjustData>).data?.inventoryAdjustQuantities);
}

/**
 * Search the store's customers.
 * @param input The search, page size and cursor.
 * @returns One page of customers.
 * @capability shopify.com/customers.search { shop: string }
 */
export function searchCustomers(input: SearchInput = {}): Page<CustomerSummary> {
    const { query, first, after } = input;
    const search = requireQuery(query);
    const window = pageWindow(first, after);
    const shop = requireShop();
    check("shopify.com/customers.search", { shop: shop });
    const customers = (graphql(shop, SEARCH_CUSTOMERS, buildSearchVariables(window, search)).json() as GraphQlResponse<CustomersData>).data?.customers;
    const items: CustomerSummary[] = [];
    for (const raw of customers?.nodes ?? []) items.push(customerSummaryFrom(raw));
    return { items: items, nextCursor: nextCursorFrom(customers?.pageInfo) };
}

/**
 * Get one customer, with their last 10 orders; null when they do not exist.
 * @param customerId Customer id.
 * @returns The customer, or null.
 * @capability shopify.com/customers.get { shop: string, customerId: string }
 */
export function getCustomer(customerId: string): Customer | null {
    const id = normalizeGid(customerId, "Customer", "customerId");
    const shop = requireShop();
    check("shopify.com/customers.get", { shop: shop, customerId: id });
    const raw = (graphql(shop, GET_CUSTOMER, { id: id }).json() as GraphQlResponse<CustomerData>).data?.customer;
    return raw === null || raw === undefined ? null : customerFrom(raw);
}

/**
 * List abandoned checkouts, newest first, each with the link the buyer can use to finish it.
 * @param input The search, page size and cursor.
 * @returns One page of abandoned checkouts.
 * @capability shopify.com/abandonedCheckouts.list { shop: string }
 */
export function listAbandonedCheckouts(input: SearchInput = {}): Page<AbandonedCheckout> {
    const { query, first, after } = input;
    const search = requireQuery(query);
    const window = pageWindow(first, after);
    const shop = requireShop();
    check("shopify.com/abandonedCheckouts.list", { shop: shop });
    const list = (graphql(shop, LIST_ABANDONED_CHECKOUTS, buildSearchVariables(window, search)).json() as GraphQlResponse<AbandonedData>).data?.abandonedCheckouts;
    const items: AbandonedCheckout[] = [];
    for (const raw of list?.nodes ?? []) items.push(abandonedFrom(raw));
    return { items: items, nextCursor: nextCursorFrom(list?.pageInfo) };
}

/**
 * Create a discount code for the whole store. In the check, the kind not used is 0 and no usage limit is 0.
 * @param input The code, its value and its limits.
 * @returns The discount.
 * @capability shopify.com/discounts.create { shop: string, code: string, percentage: number, amount: number, usageLimit: number }
 */
export function createDiscountCode(input: DiscountInput): DiscountCode {
    const { title, code, percentage, amount, startsAt, endsAt, usageLimit, oncePerCustomer, minimumSubtotal } = input;
    const name = requireText(title, "title", 255);
    const discountCode = normalizeCode(code);
    const percent = percentage === undefined ? null : normalizePercentage(percentage, "percentage");
    const fixed = amount === undefined ? null : normalizeAmount(amount, "amount");
    requireOneDiscountValue(percent, fixed);
    const starts = normalizeInstant(startsAt, "startsAt");
    const ends = endsAt === undefined ? null : normalizeInstant(endsAt, "endsAt");
    const limit = usageLimit === undefined ? null : normalizeUsageLimit(usageLimit);
    const minimum = minimumSubtotal === undefined ? null : normalizeAmount(minimumSubtotal, "minimumSubtotal");
    const shop = requireShop();
    check("shopify.com/discounts.create", {
        shop: shop,
        code: discountCode,
        percentage: percent ?? 0,
        amount: fixed ?? 0,
        usageLimit: limit ?? 0,
    });
    const discount = buildDiscount(name, discountCode, percent, fixed, starts, ends, limit, oncePerCustomer === true, minimum);
    return discountFrom((graphql(shop, DISCOUNT_CREATE, { basicCodeDiscount: discount }).json() as GraphQlResponse<DiscountCreateData>).data?.discountCodeBasicCreate);
}

/**
 * Stop a discount code from working.
 * @param discountId Discount id, from `createDiscountCode`.
 * @returns The discount.
 * @capability shopify.com/discounts.deactivate { shop: string, discountId: string }
 */
export function deactivateDiscountCode(discountId: string): DiscountCode {
    const id = normalizeGid(discountId, "DiscountCodeNode", "discountId");
    const shop = requireShop();
    check("shopify.com/discounts.deactivate", { shop: shop, discountId: id });
    return discountFrom((graphql(shop, DISCOUNT_DEACTIVATE, { id: id }).json() as GraphQlResponse<DiscountDeactivateData>).data?.discountCodeDeactivate);
}

/**
 * Create a draft order from catalog variants. Its `invoiceUrl` is a link the buyer can pay at. In the
 * check, `itemCount` is the total units, and `discountPercentage` is 0 without a discount.
 * @param input The lines, the buyer and a discount.
 * @returns The draft order.
 * @capability shopify.com/draftOrders.create { shop: string, itemCount: number, email: string, discountPercentage: number }
 */
export function createDraftOrder(input: DraftOrderInput): DraftOrder {
    const { lineItems, email, note, discount } = input;
    const lines: PreparedDraftLine[] = [];
    let units = 0;
    for (const line of lineItems) {
        const { variantId, quantity } = line;
        const prepared = prepareDraftLine(variantId, quantity);
        units = units + prepared.quantity;
        lines.push(prepared);
    }
    requireLineCount(lines.length);
    const buyer = email === undefined ? null : normalizeEmail(email);
    const memo = note === undefined ? null : normalizeNote(note);
    let percent: number | null = null;
    let label: string | null = null;
    if (discount !== undefined) {
        const { percentage, title } = discount;
        percent = normalizePercentage(percentage, "discount percentage");
        label = title === undefined ? null : requireText(title, "discount title", 255);
    }
    const shop = requireShop();
    check("shopify.com/draftOrders.create", { shop: shop, itemCount: units, email: buyer ?? "", discountPercentage: percent ?? 0 });
    const variables = { input: buildDraftOrder(lines, buyer, memo, percent, label) };
    return draftOrderFrom((graphql(shop, DRAFT_ORDER_CREATE, variables).json() as GraphQlResponse<DraftCreateData>).data?.draftOrderCreate);
}

/**
 * Email a draft order's invoice.
 * @param draftOrderId Draft order id.
 * @param to Recipient's email; trimmed and lowercased.
 * @param subject Email subject.
 * @returns The draft order.
 * @capability shopify.com/draftOrders.sendInvoice { shop: string, draftOrderId: string, to: string }
 */
export function sendDraftOrderInvoice(draftOrderId: string, to: string, subject?: string): DraftOrder {
    const id = normalizeGid(draftOrderId, "DraftOrder", "draftOrderId");
    const recipient = normalizeEmail(to);
    const heading = subject === undefined ? null : requireText(subject, "subject", 255);
    const shop = requireShop();
    check("shopify.com/draftOrders.sendInvoice", { shop: shop, draftOrderId: id, to: recipient });
    const email: Record<string, unknown> = { to: recipient };
    if (heading !== null) email["subject"] = heading;
    return draftOrderFrom((graphql(shop, DRAFT_ORDER_SEND_INVOICE, { id: id, email: email }).json() as GraphQlResponse<DraftSendData>).data?.draftOrderInvoiceSend);
}

/**
 * Get the store's name, currency, time zone and plan.
 * @returns The store.
 * @capability shopify.com/shop.get { shop: string }
 */
export function getShop(): Shop {
    const shop = requireShop();
    check("shopify.com/shop.get", { shop: shop });
    return shopFrom((graphql(shop, GET_SHOP, {}).json() as GraphQlResponse<ShopData>).data?.shop);
}

function trackingFrom(input: TrackingInput): Record<string, unknown> {
    const { company, number, url } = input;
    const carrier = company === undefined ? null : requireText(company, "tracking company", 255);
    const trackingNumber = number === undefined ? null : requireText(number, "tracking number", 255);
    const link = url === undefined ? null : normalizeHttpsUrl(url);
    if (carrier === null && trackingNumber === null && link === null) {
        throw validationError("empty_update", "name at least one tracking field");
    }
    return buildTracking(carrier, trackingNumber, link);
}

function inventoryWritten(result: RawInventoryWrite | null | undefined): void {
    requireNoUserErrors(result?.userErrors);
    if (result === null || result === undefined) throw validationError("unexpected_response", "Shopify sent no inventory result");
}

// Internal: no check here. The exported operation checks what this resolved.
function requireOrderFacts(shop: string, orderId: string): RawOrder {
    const order = (graphql(shop, ORDER_FACTS, { id: orderId }).json() as GraphQlResponse<OrderData>).data?.order;
    if (order === null || order === undefined) throw validationError("not_found", "no order " + orderId);
    return order;
}

function fetchSuggestion(shop: string, orderId: string, lines: Record<string, unknown>[], refundShipping: boolean): SuggestedRefund | null {
    const variables: Record<string, unknown> = { id: orderId, refundShipping: refundShipping };
    if (lines.length > 0) variables["refundLineItems"] = lines;
    const order = (graphql(shop, SUGGEST_REFUND, variables).json() as GraphQlResponse<SuggestionData>).data?.order;
    if (order === null || order === undefined) return null;
    const raw = order.suggestedRefund;
    if (raw === null || raw === undefined) throw validationError("unexpected_response", "Shopify sent no suggested refund");
    return suggestedRefundFrom(raw);
}

function requireShop(): string {
    const shop = secrets.get("SHOPIFY_SHOP");
    if (shop === undefined) throw validationError("missing_shop", "SHOPIFY_SHOP is not bound");
    return normalizeShop(shop);
}

function authHeaders(): Map<string, string> {
    const token = secrets.get("SHOPIFY_ACCESS_TOKEN");
    if (token === undefined) throw validationError("missing_access_token", "SHOPIFY_ACCESS_TOKEN is not bound");
    const headers = new Map<string, string>();
    headers.set("X-Shopify-Access-Token", token);
    headers.set("Accept", "application/json");
    headers.set("User-Agent", "submilli-shopify-admin/0.1.0");
    return headers;
}

// The one request. A GraphQL error comes back with HTTP 200 and is thrown here,
// so callers read a body that holds no `errors`.
function graphql(shop: string, query: string, variables: Record<string, unknown>): Response {
    const response = post("https://" + shop + GRAPHQL_PATH, { query: query, variables: variables }, authHeaders());
    if (!response.ok) {
        throw httpFailure(response.status, response.statusText, response.body, retryAfterFrom(response.headers.get("retry-after") ?? ""));
    }
    const failure = graphQlFailure(response.json() as GraphQlStatus);
    if (failure !== null) throw failure;
    return response;
}
