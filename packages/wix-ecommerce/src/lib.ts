// A synchronous, typed client for the Wix eCommerce, Stores and Contacts REST
// APIs at https://www.wixapis.com. The API key and the site id are read from the
// WIX_API_KEY and WIX_SITE_ID secrets inside the package; neither is an argument
// and the key is never returned. Every operation normalizes its input, resolves
// what a policy needs from Wix (an order's total, a payment's currency), checks
// those values, and sends exactly those values.
//
// The site id is in every check, so a policy can pin one site. There are no
// delete operations, and an order or a product cannot be created.

import { get, patch, post, Response } from "submilli:http";
import { encodeComponent } from "submilli:url";
import secrets from "submilli:secrets";
import { check } from "submilli:security";
import { validationError, WixError } from "./errors";
import {
    AbandonedCheckoutFilter,
    buildAbandonedQuery,
    buildAdjustV1,
    buildAdjustV3,
    buildCancelBody,
    buildCartBody,
    buildContactQuery,
    buildFulfillmentBody,
    buildInventoryQueryV1,
    buildInventoryQueryV3,
    buildNoteBody,
    buildOrderSearch,
    buildProductPatchV1,
    buildProductPatchV3,
    buildProductQueryV1,
    buildProductSearchV3,
    buildRefundBody,
    buildTracking,
    CancelInput,
    CartCustomer,
    CartItemInput,
    FulfillmentInput,
    normalizeAmount,
    normalizeDelta,
    normalizeEmail,
    normalizeId,
    normalizeNote,
    normalizeProvider,
    normalizeSiteId,
    normalizeTrackingLink,
    normalizeTrackingNumber,
    offsetFrom,
    optionalMessage,
    optionalReason,
    OrderQuery,
    PageOptions,
    pageWindow,
    PreparedLine,
    prepareCartItem,
    prepareLine,
    prepareRefundLine,
    PreparedCartItem,
    ProductQuery,
    productFields,
    ProductUpdate,
    RefundInput,
    requireLineCount,
    requireSearchText,
    requireText,
    TrackingUpdate,
} from "./request";
import {
    AbandonedCheckout,
    AbandonedEnvelope,
    abandonedFrom,
    AbandonedList,
    Cart,
    CartEnvelope,
    cartFrom,
    CheckoutUrlEnvelope,
    Contact,
    ContactEnvelope,
    contactFrom,
    ContactList,
    failureFrom,
    Fulfillment,
    FulfillmentsEnvelope,
    fulfillmentsFrom,
    InventoryLevel,
    inventoryFromV1,
    inventoryFromV3,
    InventoryV1List,
    InventoryV3List,
    nextCursorFrom,
    Order,
    OrderEnvelope,
    orderFrom,
    OrderSearchResult,
    Page,
    Payment,
    paymentsFrom,
    Product,
    productFromV1,
    productFromV3,
    ProductV1Envelope,
    ProductV1List,
    ProductV3Envelope,
    ProductV3List,
    RawOrder,
    Refund,
    RefundEnvelope,
    refundFrom,
    requireAmount,
    retryAfterFrom,
    TransactionsEnvelope,
    VersionEnvelope,
} from "./response";

export { WixError } from "./errors";
export {
    AbandonedCheckoutFilter,
    CancelInput,
    CartCustomer,
    CartItemInput,
    FulfillmentInput,
    LineItemQuantity,
    OrderQuery,
    OrderSort,
    PageOptions,
    ProductQuery,
    ProductUpdate,
    RefundInput,
    TrackingInput,
    TrackingUpdate,
} from "./request";
export {
    AbandonedCheckout,
    Cart,
    Contact,
    Fulfillment,
    InventoryLevel,
    Order,
    OrderLine,
    Page,
    Payment,
    Product,
    Refund,
    Tracking,
} from "./response";

const API = "https://www.wixapis.com/";
const V1_CATALOG = "V1_CATALOG";
const V3_CATALOG = "V3_CATALOG";
const V3_PRODUCT_FIELDS = "?fields=CURRENCY&fields=PLAIN_DESCRIPTION";

/**
 * Search the site's orders, newest first unless `query.sort` says otherwise.
 * @param query Filters and sort; omit it for every order Wix returns by default.
 * @param page Page size and cursor.
 * @returns One page of orders.
 * @capability wix.com/orders.search { siteId: string }
 */
export function searchOrders(query: OrderQuery = {}, page: PageOptions = {}): Page<Order> {
    const window = pageWindow(page);
    const body = buildOrderSearch(query, window);
    const siteId = requireSiteId();
    check("wix.com/orders.search", { siteId: siteId });
    const result = wixPost(siteId, "ecom/v1/orders/search", body).json() as OrderSearchResult;
    const items: Order[] = [];
    for (const raw of result.orders ?? []) items.push(orderFrom(raw));
    return { items: items, nextCursor: nextCursorFrom(result.metadata) };
}

/**
 * Get one order; null when it does not exist.
 * @param orderId Order id.
 * @returns The order, or null.
 * @capability wix.com/orders.get { siteId: string, orderId: string }
 */
export function getOrder(orderId: string): Order | null {
    const id = normalizeId(orderId, "orderId");
    const siteId = requireSiteId();
    check("wix.com/orders.get", { siteId: siteId, orderId: id });
    const raw = fetchOrder(siteId, id);
    return raw === null ? null : orderFrom(raw);
}

/**
 * Add a note for the merchant to an order. Buyers do not see it.
 * @param orderId Order id.
 * @param message The note, 1-5000 characters.
 * @capability wix.com/orders.note { siteId: string, orderId: string }
 */
export function addOrderNote(orderId: string, message: string): void {
    const id = normalizeId(orderId, "orderId");
    const note = normalizeNote(message);
    const siteId = requireSiteId();
    check("wix.com/orders.note", { siteId: siteId, orderId: id });
    wixPost(siteId, "ecom/v1/orders/" + encodeComponent(id) + "/activities/add", buildNoteBody(note));
}

/**
 * Cancel an order. The order is fetched first, so a policy sees its real total and payment status.
 * @param orderId Order id.
 * @param input Whether to restock, whether to email the buyer, and a note for that email.
 * @returns The canceled order.
 * @capability wix.com/orders.cancel { siteId: string, orderId: string, totalAmount: number, paymentStatus: string, restock: boolean }
 */
export function cancelOrder(orderId: string, input: CancelInput = {}): Order {
    const { restock, notifyCustomer, message } = input;
    const id = normalizeId(orderId, "orderId");
    const shouldRestock = restock === true;
    const notify = notifyCustomer === true;
    const note = optionalMessage(message, notify);
    const siteId = requireSiteId();
    const order = requireOrder(siteId, id);
    check("wix.com/orders.cancel", {
        siteId: siteId,
        orderId: id,
        totalAmount: requireAmount(order.priceSummary?.total?.amount, "order total"),
        paymentStatus: order.paymentStatus ?? "",
        restock: shouldRestock,
    });
    const body = buildCancelBody(shouldRestock, notify, note);
    const envelope = wixPost(siteId, "ecom/v1/orders/" + encodeComponent(id) + "/cancel", body).json() as OrderEnvelope;
    return orderFrom(envelope.order ?? order);
}

/**
 * List an order's fulfillments.
 * @param orderId Order id.
 * @returns The fulfillments, oldest first as Wix returns them.
 * @capability wix.com/fulfillments.list { siteId: string, orderId: string }
 */
export function listFulfillments(orderId: string): Fulfillment[] {
    const id = normalizeId(orderId, "orderId");
    const siteId = requireSiteId();
    check("wix.com/fulfillments.list", { siteId: siteId, orderId: id });
    return fulfillmentsFrom(wixGet(siteId, "ecom/v1/fulfillments/orders/" + encodeComponent(id)).json() as FulfillmentsEnvelope);
}

/**
 * Record a fulfillment with tracking. Wix may email the buyer, depending on the site's settings. The order must be `APPROVED`.
 * @param orderId Order id.
 * @param input Tracking, and the lines to fulfill; every line when `lineItems` is omitted.
 * @returns All of the order's fulfillments, including the new one.
 * @capability wix.com/fulfillments.create { siteId: string, orderId: string, shippingProvider: string }
 */
export function createFulfillment(orderId: string, input: FulfillmentInput): Fulfillment[] {
    const { tracking, lineItems } = input;
    const { number, provider, link } = tracking;
    const id = normalizeId(orderId, "orderId");
    const trackingNumber = normalizeTrackingNumber(number);
    const carrier = normalizeProvider(provider);
    const trackingLink = link === undefined ? null : normalizeTrackingLink(link);
    const chosen: PreparedLine[] = [];
    for (const line of lineItems ?? []) {
        const { lineItemId, quantity } = line;
        chosen.push(prepareLine(lineItemId, quantity));
    }
    if (lineItems !== undefined) requireLineCount(chosen.length);
    const siteId = requireSiteId();
    check("wix.com/fulfillments.create", { siteId: siteId, orderId: id, shippingProvider: carrier });
    const lines = chosen.length > 0 ? chosen : allLines(requireOrder(siteId, id));
    const body = buildFulfillmentBody(lines, buildTracking(trackingNumber, carrier, trackingLink));
    const path = "ecom/v1/fulfillments/orders/" + encodeComponent(id) + "/create-fulfillment";
    return fulfillmentsFrom(wixPost(siteId, path, body).json() as FulfillmentsEnvelope);
}

/**
 * Change the tracking on a fulfillment.
 * @param orderId Order id.
 * @param fulfillmentId Fulfillment id.
 * @param tracking The details to change.
 * @returns All of the order's fulfillments.
 * @capability wix.com/fulfillments.update { siteId: string, orderId: string, fulfillmentId: string }
 */
export function updateTracking(orderId: string, fulfillmentId: string, tracking: TrackingUpdate): Fulfillment[] {
    const { number, provider, link } = tracking;
    const id = normalizeId(orderId, "orderId");
    const fulfillment = normalizeId(fulfillmentId, "fulfillmentId");
    const trackingNumber = number === undefined ? null : normalizeTrackingNumber(number);
    const carrier = provider === undefined ? null : normalizeProvider(provider);
    const trackingLink = link === undefined ? null : normalizeTrackingLink(link);
    if (trackingNumber === null && carrier === null && trackingLink === null) {
        throw validationError("empty_update", "name at least one tracking field to change");
    }
    const siteId = requireSiteId();
    check("wix.com/fulfillments.update", { siteId: siteId, orderId: id, fulfillmentId: fulfillment });
    const body: Record<string, unknown> = { fulfillment: { trackingInfo: buildTracking(trackingNumber, carrier, trackingLink) } };
    const path = "ecom/v1/fulfillments/" + encodeComponent(fulfillment) + "/orders/" + encodeComponent(id);
    return fulfillmentsFrom(wixPatch(siteId, path, body).json() as FulfillmentsEnvelope);
}

/**
 * List an order's payments, with what is left to refund on each.
 * @param orderId Order id.
 * @returns The payments.
 * @capability wix.com/payments.list { siteId: string, orderId: string }
 */
export function listPayments(orderId: string): Payment[] {
    const id = normalizeId(orderId, "orderId");
    const siteId = requireSiteId();
    check("wix.com/payments.list", { siteId: siteId, orderId: id });
    return fetchPayments(siteId, id);
}

/**
 * Refund part or all of one payment. The order and its payments are fetched first, so a policy sees the
 * real amount and currency, and an amount above what is left on the payment is refused with no refund.
 * @param orderId Order id.
 * @param input The payment, the amount, and what else to do.
 * @returns The refund.
 * @capability wix.com/orders.refund { siteId: string, orderId: string, amount: number, currency: string, restock: boolean, externalRefund: boolean }
 */
export function refundOrder(orderId: string, input: RefundInput): Refund {
    const { paymentId, amount, lineItems, restock, notifyCustomer, message, reason, externalRefund } = input;
    const id = normalizeId(orderId, "orderId");
    const payment = normalizeId(paymentId, "paymentId");
    const refundAmount = normalizeAmount(amount, "amount");
    const shouldRestock = restock === true;
    const external = externalRefund === true;
    const notify = notifyCustomer === true;
    const note = optionalMessage(message, notify);
    const recordedReason = optionalReason(reason);
    const lines: PreparedLine[] = [];
    for (const line of lineItems ?? []) {
        const { lineItemId, quantity } = line;
        lines.push(prepareRefundLine(lineItemId, quantity));
    }
    if (lineItems !== undefined) requireLineCount(lines.length);
    if (shouldRestock && lines.length === 0) {
        throw validationError("restock_needs_line_items", "name the lineItems to restock");
    }
    const siteId = requireSiteId();
    const order = requireOrder(siteId, id);
    const currency = order.currency ?? "";
    if (currency.length === 0) throw validationError("unexpected_response", "Wix sent no order currency");
    const paid = findPayment(fetchPayments(siteId, id), payment);
    if (paid.refundDisabled) throw validationError("refund_disabled", "Wix does not allow refunds of payment " + payment);
    if (refundAmount > paid.refundable) {
        throw validationError("refund_too_large", "amount " + refundAmount.toString() + " is above the " + paid.refundable.toString() + " left on payment " + payment);
    }
    check("wix.com/orders.refund", {
        siteId: siteId,
        orderId: id,
        amount: refundAmount,
        currency: currency,
        restock: shouldRestock,
        externalRefund: external,
    });
    const body = buildRefundBody(id, payment, refundAmount, lines, shouldRestock, external, notify, note, recordedReason);
    return refundFrom((wixPost(siteId, "ecom/v1/order-billing/refund-payments", body).json() as RefundEnvelope).refund);
}

/**
 * List abandoned checkouts, newest first.
 * @param filter Filters; omit it for every abandoned checkout.
 * @param page Page size and cursor.
 * @returns One page of abandoned checkouts.
 * @capability wix.com/abandonedCheckouts.list { siteId: string }
 */
export function listAbandonedCheckouts(filter: AbandonedCheckoutFilter = {}, page: PageOptions = {}): Page<AbandonedCheckout> {
    const window = pageWindow(page);
    const body = buildAbandonedQuery(filter, window);
    const siteId = requireSiteId();
    check("wix.com/abandonedCheckouts.list", { siteId: siteId });
    const list = wixPost(siteId, "ecom/v1/abandoned-checkout/query", body).json() as AbandonedList;
    const items: AbandonedCheckout[] = [];
    for (const raw of list.abandonedCheckouts ?? []) items.push(abandonedFrom(raw));
    return { items: items, nextCursor: nextCursorFrom(list.metadata ?? list.pagingMetadata) };
}

/**
 * Get one abandoned checkout, with the link the buyer can use to finish it; null when it does not exist.
 * @param abandonedCheckoutId Abandoned checkout id.
 * @returns The abandoned checkout, or null.
 * @capability wix.com/abandonedCheckouts.get { siteId: string, abandonedCheckoutId: string }
 */
export function getAbandonedCheckout(abandonedCheckoutId: string): AbandonedCheckout | null {
    const id = normalizeId(abandonedCheckoutId, "abandonedCheckoutId");
    const siteId = requireSiteId();
    check("wix.com/abandonedCheckouts.get", { siteId: siteId, abandonedCheckoutId: id });
    const response = wixGetNullable(siteId, "ecom/v1/abandoned-checkout/" + encodeComponent(id));
    if (response === null) return null;
    const raw = (response.json() as AbandonedEnvelope).abandonedCheckout;
    return raw === null || raw === undefined ? null : abandonedFrom(raw);
}

/**
 * Create a cart of Wix Stores products, ready for `getCheckoutUrl`.
 * @param items Products and quantities, 1-300 lines.
 * @param customer Who the cart is for.
 * @returns The new cart.
 * @capability wix.com/carts.create { siteId: string, itemCount: number }
 */
export function createCart(items: CartItemInput[], customer: CartCustomer = {}): Cart {
    const { email, firstName, lastName } = customer;
    const prepared: PreparedCartItem[] = [];
    let units = 0;
    for (const item of items) {
        const { productId, quantity, variantId } = item;
        const line = prepareCartItem(productId, quantity, variantId);
        units = units + line.quantity;
        prepared.push(line);
    }
    requireLineCount(prepared.length);
    const address = email === undefined ? null : normalizeEmail(email);
    const first = firstName === undefined ? null : requireText(firstName, "firstName", 100);
    const last = lastName === undefined ? null : requireText(lastName, "lastName", 100);
    const siteId = requireSiteId();
    check("wix.com/carts.create", { siteId: siteId, itemCount: units });
    return cartFrom(wixPost(siteId, "ecom/v2/carts", buildCartBody(prepared, address, first, last)).json() as CartEnvelope);
}

/**
 * Get the checkout link for a cart.
 * @param cartId Cart id.
 * @returns A link the buyer can open to pay.
 * @capability wix.com/carts.checkoutUrl { siteId: string, cartId: string }
 */
export function getCheckoutUrl(cartId: string): string {
    const id = normalizeId(cartId, "cartId");
    const siteId = requireSiteId();
    check("wix.com/carts.checkoutUrl", { siteId: siteId, cartId: id });
    const body: Record<string, unknown> = {};
    const url = (wixPost(siteId, "ecom/v2/carts/" + encodeComponent(id) + "/get-checkout-url", body).json() as CheckoutUrlEnvelope).checkoutUrl ?? "";
    if (url.length === 0) throw validationError("unexpected_response", "Wix sent no checkout link");
    return url;
}

/**
 * Get which Stores catalog the site uses. The two catalogs are not compatible; the product and inventory operations follow it.
 * @returns `V1_CATALOG`, `V3_CATALOG` or `STORES_NOT_INSTALLED`.
 * @capability wix.com/catalog.version { siteId: string }
 */
export function getCatalogVersion(): string {
    const siteId = requireSiteId();
    check("wix.com/catalog.version", { siteId: siteId });
    return fetchCatalogVersion(siteId);
}

/**
 * Search the catalog. Hidden products are left out unless asked for.
 * @param query Search text, and whether to include hidden products.
 * @param page Page size and cursor.
 * @returns One page of products.
 * @capability wix.com/products.search { siteId: string, includeHidden: boolean }
 */
export function searchProducts(query: ProductQuery = {}, page: PageOptions = {}): Page<Product> {
    const { text, includeHidden } = query;
    const hidden = includeHidden === true;
    const search = requireSearchText(text);
    const window = pageWindow(page);
    const siteId = requireSiteId();
    check("wix.com/products.search", { siteId: siteId, includeHidden: hidden });
    const items: Product[] = [];
    if (requireCatalog(siteId) === V1_CATALOG) {
        const offset = offsetFrom(window.cursor);
        const list = wixPost(siteId, "stores/v1/products/query", buildProductQueryV1(search, hidden, window.limit, offset)).json() as ProductV1List;
        for (const raw of list.products ?? []) items.push(productFromV1(raw));
        const seen = offset + items.length;
        const hasMore = list.totalResults === null || list.totalResults === undefined ? items.length === window.limit : list.totalResults > seen;
        return { items: items, nextCursor: hasMore ? seen.toString() : null };
    }
    const list = wixPost(siteId, "stores/v3/products/search", buildProductSearchV3(search, hidden, window)).json() as ProductV3List;
    for (const raw of list.products ?? []) items.push(productFromV3(raw));
    return { items: items, nextCursor: nextCursorFrom(list.pagingMetadata) };
}

/**
 * Get one product; null when it does not exist.
 * @param productId Product id.
 * @returns The product, or null.
 * @capability wix.com/products.get { siteId: string, productId: string }
 */
export function getProduct(productId: string): Product | null {
    const id = normalizeId(productId, "productId");
    const siteId = requireSiteId();
    check("wix.com/products.get", { siteId: siteId, productId: id });
    if (requireCatalog(siteId) === V1_CATALOG) {
        const response = wixGetNullable(siteId, "stores/v1/products/" + encodeComponent(id));
        if (response === null) return null;
        const raw = (response.json() as ProductV1Envelope).product;
        return raw === null || raw === undefined ? null : productFromV1(raw);
    }
    const response = wixGetNullable(siteId, "stores/v3/products/" + encodeComponent(id) + V3_PRODUCT_FIELDS);
    if (response === null) return null;
    const raw = (response.json() as ProductV3Envelope).product;
    return raw === null || raw === undefined ? null : productFromV3(raw);
}

/**
 * Change a product. The check lists the field names set, so a policy can forbid `price`. `ribbon` and
 * `price` work on catalog V1 only; on V3 they are refused with `unsupported_on_catalog`.
 * @param productId Product id.
 * @param fields The fields to change.
 * @returns The updated product.
 * @capability wix.com/products.update { siteId: string, productId: string, fields: string[] }
 */
export function updateProduct(productId: string, fields: ProductUpdate): Product {
    const { name, description, visible, ribbon, price } = fields;
    const id = normalizeId(productId, "productId");
    const newName = name === undefined ? null : requireText(name, "name", 80);
    const newDescription = description === undefined ? null : requireText(description, "description", 16000);
    const newVisible = visible === undefined ? null : visible;
    const newRibbon = ribbon === undefined ? null : requireText(ribbon, "ribbon", 30);
    const newPrice = price === undefined ? null : normalizeAmount(price, "price");
    const names = productFields(newName !== null, newDescription !== null, newVisible !== null, newRibbon !== null, newPrice !== null);
    const siteId = requireSiteId();
    check("wix.com/products.update", { siteId: siteId, productId: id, fields: names });
    const path = encodeComponent(id);
    if (requireCatalog(siteId) === V1_CATALOG) {
        const body = buildProductPatchV1(id, newName, newDescription, newVisible, newRibbon, newPrice);
        const raw = (wixPatch(siteId, "stores/v1/products/" + path, body).json() as ProductV1Envelope).product;
        if (raw === null || raw === undefined) throw validationError("unexpected_response", "Wix sent no product");
        return productFromV1(raw);
    }
    if (newRibbon !== null || newPrice !== null) {
        throw validationError("unsupported_on_catalog", "ribbon and price cannot be changed here on catalog V3");
    }
    const current = (wixGet(siteId, "stores/v3/products/" + path + V3_PRODUCT_FIELDS).json() as ProductV3Envelope).product;
    const revision = current?.revision ?? "";
    if (revision.length === 0) throw validationError("unexpected_response", "Wix sent no product revision");
    const body = buildProductPatchV3(id, revision, newName, newDescription, newVisible);
    const raw = (wixPatch(siteId, "stores/v3/products/" + path, body).json() as ProductV3Envelope).product;
    if (raw === null || raw === undefined) throw validationError("unexpected_response", "Wix sent no product");
    return productFromV3(raw);
}

/**
 * Get a product's stock, one entry per variant, and per location on catalog V3. Also the way to find variant ids.
 * @param productId Product id.
 * @returns The stock levels.
 * @capability wix.com/inventory.get { siteId: string, productId: string }
 */
export function getInventory(productId: string): InventoryLevel[] {
    const id = normalizeId(productId, "productId");
    const siteId = requireSiteId();
    check("wix.com/inventory.get", { siteId: siteId, productId: id });
    return fetchInventory(siteId, id, requireCatalog(siteId));
}

/**
 * Add to or take from a variant's stock. Inventory must be tracked by quantity. On catalog V3 a
 * variant stocked in several locations is refused with `ambiguous_inventory_item`.
 * @param productId Product id.
 * @param variantId Variant id, from `getInventory`.
 * @param delta Change in stock: positive adds, negative removes; not zero.
 * @capability wix.com/inventory.adjust { siteId: string, productId: string, variantId: string, delta: number }
 */
export function adjustInventory(productId: string, variantId: string, delta: number): void {
    const id = normalizeId(productId, "productId");
    const variant = normalizeId(variantId, "variantId");
    const change = normalizeDelta(delta);
    const siteId = requireSiteId();
    check("wix.com/inventory.adjust", { siteId: siteId, productId: id, variantId: variant, delta: change });
    const catalog = requireCatalog(siteId);
    const verb = change > 0 ? "increment" : "decrement";
    if (catalog === V1_CATALOG) {
        wixPost(siteId, "stores/v2/inventoryItems/" + verb, buildAdjustV1(id, variant, change));
        return;
    }
    let inventoryItemId: string | null = null;
    for (const level of fetchInventory(siteId, id, catalog)) {
        if (level.variantId !== variant) continue;
        if (inventoryItemId !== null) {
            throw validationError("ambiguous_inventory_item", "variant " + variant + " is stocked in several locations");
        }
        inventoryItemId = level.inventoryItemId;
    }
    if (inventoryItemId === null) throw validationError("not_found", "no inventory item for variant " + variant);
    wixPost(siteId, "stores/v3/bulk/inventory-items/" + verb, buildAdjustV3(inventoryItemId, change));
}

/**
 * Find the contact with an email address; null when nobody has it.
 * @param email Email address; trimmed and lowercased before the lookup.
 * @returns The contact, or null.
 * @capability wix.com/contacts.find { siteId: string, email: string }
 */
export function findContactByEmail(email: string): Contact | null {
    const address = normalizeEmail(email);
    const siteId = requireSiteId();
    check("wix.com/contacts.find", { siteId: siteId, email: address });
    const contacts = (wixPost(siteId, "contacts/v4/contacts/query", buildContactQuery(address)).json() as ContactList).contacts ?? [];
    if (contacts.length === 0) return null;
    if (contacts.length > 1) throw validationError("multiple_matches", "more than one contact has the email " + address);
    return contactFrom(contacts[0]);
}

/**
 * Get one contact; null when it does not exist.
 * @param contactId Contact id, such as `order.contactId`.
 * @returns The contact, or null.
 * @capability wix.com/contacts.get { siteId: string, contactId: string }
 */
export function getContact(contactId: string): Contact | null {
    const id = normalizeId(contactId, "contactId");
    const siteId = requireSiteId();
    check("wix.com/contacts.get", { siteId: siteId, contactId: id });
    const response = wixGetNullable(siteId, "contacts/v4/contacts/" + encodeComponent(id) + "?fieldsets=FULL");
    if (response === null) return null;
    const raw = (response.json() as ContactEnvelope).contact;
    return raw === null || raw === undefined ? null : contactFrom(raw);
}

function allLines(order: RawOrder): PreparedLine[] {
    const lines: PreparedLine[] = [];
    for (const line of order.lineItems ?? []) lines.push({ lineItemId: line.id ?? "", quantity: null });
    return lines;
}

function findPayment(payments: Payment[], paymentId: string): Payment {
    for (const payment of payments) {
        if (payment.id === paymentId) return payment;
    }
    throw validationError("payment_not_found", "payment " + paymentId + " is not on this order");
}

// Internal: no check here. The exported operation checks what this resolved.
function fetchOrder(siteId: string, orderId: string): RawOrder | null {
    const response = wixGetNullable(siteId, "ecom/v1/orders/" + encodeComponent(orderId));
    if (response === null) return null;
    return (response.json() as OrderEnvelope).order ?? null;
}

function requireOrder(siteId: string, orderId: string): RawOrder {
    const order = fetchOrder(siteId, orderId);
    if (order === null) throw validationError("not_found", "no order " + orderId);
    return order;
}

function fetchPayments(siteId: string, orderId: string): Payment[] {
    return paymentsFrom(wixGet(siteId, "ecom/v1/payments/orders/" + encodeComponent(orderId)).json() as TransactionsEnvelope);
}

function fetchInventory(siteId: string, productId: string, catalog: string): InventoryLevel[] {
    if (catalog === V1_CATALOG) {
        return inventoryFromV1(wixPost(siteId, "stores/v2/inventoryItems/query", buildInventoryQueryV1(productId)).json() as InventoryV1List);
    }
    return inventoryFromV3(wixPost(siteId, "stores/v3/inventory-items/query", buildInventoryQueryV3(productId)).json() as InventoryV3List);
}

function fetchCatalogVersion(siteId: string): string {
    const version = (wixGet(siteId, "stores/v3/provision/version").json() as VersionEnvelope).catalogVersion ?? "";
    if (version.length === 0) throw validationError("unexpected_response", "Wix sent no catalog version");
    return version;
}

function requireCatalog(siteId: string): string {
    const version = fetchCatalogVersion(siteId);
    if (version !== V1_CATALOG && version !== V3_CATALOG) {
        throw validationError("stores_not_installed", "this site has no Stores catalog: " + version);
    }
    return version;
}

function requireSiteId(): string {
    const siteId = secrets.get("WIX_SITE_ID");
    if (siteId === undefined) throw validationError("missing_site_id", "WIX_SITE_ID is not bound");
    return normalizeSiteId(siteId);
}

function wixGet(siteId: string, path: string): Response {
    return requireOk(get(API + path, authHeaders(siteId)));
}

function wixGetNullable(siteId: string, path: string): Response | null {
    const response = get(API + path, authHeaders(siteId));
    if (response.ok) return response;
    if (response.status === 404) return null;
    throw wixError(response);
}

function wixPost(siteId: string, path: string, body: Record<string, unknown>): Response {
    return requireOk(post(API + path, body, authHeaders(siteId)));
}

function wixPatch(siteId: string, path: string, body: Record<string, unknown>): Response {
    return requireOk(patch(API + path, body, authHeaders(siteId)));
}

function authHeaders(siteId: string): Map<string, string> {
    const key = secrets.get("WIX_API_KEY");
    if (key === undefined) throw validationError("missing_api_key", "WIX_API_KEY is not bound");
    const headers = new Map<string, string>();
    // A Wix API key goes in Authorization as it is, without "Bearer".
    headers.set("Authorization", key);
    headers.set("wix-site-id", siteId);
    headers.set("Accept", "application/json");
    headers.set("User-Agent", "submilli-wix-ecommerce/0.1.0");
    return headers;
}

function requireOk(response: Response): Response {
    if (response.ok) return response;
    throw wixError(response);
}

function wixError(response: Response): WixError {
    const failure = failureFrom(response.status, response.statusText, response.body);
    const retryable = response.status === 429;
    const wait = retryable ? retryAfterFrom(response.headers.get("retry-after") ?? "") : null;
    return new WixError(failure.code, failure.message, response.status, retryable, wait);
}
