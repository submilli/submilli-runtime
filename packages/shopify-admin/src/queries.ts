// The GraphQL documents, one per request. Internal.
//
// Shopify refuses a single query that costs more than 1000 points, and a
// connection costs its page size times what each node selects. The sizes here
// keep the nested selections under that limit: a query with a connection inside
// a connection is the one to re-check when a field is added.

const MONEY = "presentmentMoney { amount currencyCode }";
const PAGE_INFO = "pageInfo { hasNextPage endCursor }";
const USER_ERRORS = "userErrors { field message }";
const CODED_USER_ERRORS = "userErrors { field message code }";

const ORDER_SUMMARY = `id name createdAt cancelledAt displayFinancialStatus displayFulfillmentStatus presentmentCurrencyCode email currentTotalPriceSet { ${MONEY} }`;
const FULFILLMENT = "id status trackingInfo(first: 5) { company number url }";
const PRODUCT_SUMMARY = "id title handle status vendor productType tags totalInventory priceRangeV2 { minVariantPrice { amount currencyCode } }";
const CUSTOMER_SUMMARY = "id firstName lastName defaultEmailAddress { emailAddress } defaultPhoneNumber { phoneNumber } numberOfOrders";
const DRAFT_ORDER = `id name status invoiceUrl totalPriceSet { ${MONEY} }`;

export const SEARCH_ORDERS = `query SearchOrders($first: Int!, $after: String, $query: String, $sortKey: OrderSortKeys, $reverse: Boolean) {
    orders(first: $first, after: $after, query: $query, sortKey: $sortKey, reverse: $reverse) { nodes { ${ORDER_SUMMARY} } ${PAGE_INFO} }
}`;

export const GET_ORDER = `query GetOrder($id: ID!) {
    order(id: $id) {
        ${ORDER_SUMMARY} note tags
        lineItems(first: 100) {
            nodes { id title quantity refundableQuantity unfulfilledQuantity sku variant { id } discountedUnitPriceSet { ${MONEY} } }
            pageInfo { hasNextPage }
        }
    }
}`;

export const ORDER_FACTS = `query OrderFacts($id: ID!) {
    order(id: $id) { id cancelledAt note tags currentTotalPriceSet { ${MONEY} } }
}`;

export const ORDER_UPDATE = `mutation OrderUpdate($input: OrderInput!) {
    orderUpdate(input: $input) { order { id note tags } ${USER_ERRORS} }
}`;

export const ORDER_CANCEL = `mutation OrderCancel($orderId: ID!, $reason: OrderCancelReason!, $restock: Boolean!, $refundMethod: OrderCancelRefundMethodInput, $notifyCustomer: Boolean, $staffNote: String) {
    orderCancel(orderId: $orderId, reason: $reason, restock: $restock, refundMethod: $refundMethod, notifyCustomer: $notifyCustomer, staffNote: $staffNote) {
        job { id done }
        orderCancelUserErrors { field message code }
    }
}`;

export const SUGGEST_REFUND = `query SuggestRefund($id: ID!, $refundLineItems: [RefundLineItemInput!], $refundShipping: Boolean) {
    order(id: $id) {
        id
        suggestedRefund(refundLineItems: $refundLineItems, refundShipping: $refundShipping) {
            amountSet { ${MONEY} }
            maximumRefundableSet { ${MONEY} }
            suggestedTransactions { gateway amountSet { ${MONEY} } parentTransaction { id } }
        }
    }
}`;

export const REFUND_CREATE = `mutation RefundCreate($input: RefundInput!, $idempotencyKey: String!) {
    refundCreate(input: $input) @idempotent(key: $idempotencyKey) {
        refund { id totalRefundedSet { ${MONEY} } }
        ${USER_ERRORS}
    }
}`;

export const LIST_FULFILLMENT_ORDERS = `query ListFulfillmentOrders($id: ID!) {
    order(id: $id) {
        id
        fulfillmentOrders(first: 25) {
            nodes {
                id status
                assignedLocation { name location { id } }
                fulfillmentHolds { id reason }
                lineItems(first: 20) { nodes { id productTitle remainingQuantity totalQuantity } pageInfo { hasNextPage } }
            }
        }
    }
}`;

export const OPEN_FULFILLMENT_ORDERS = `query OpenFulfillmentOrders($id: ID!) {
    order(id: $id) { id fulfillmentOrders(first: 50) { nodes { id status assignedLocation { location { id } } } } }
}`;

export const FULFILLMENT_CREATE = `mutation FulfillmentCreate($fulfillment: FulfillmentInput!) {
    fulfillmentCreate(fulfillment: $fulfillment) { fulfillment { ${FULFILLMENT} } ${USER_ERRORS} }
}`;

export const FULFILLMENT_ORDER_OF = `query FulfillmentOwner($id: ID!) {
    fulfillment(id: $id) { id order { id } }
}`;

export const FULFILLMENT_TRACKING_UPDATE = `mutation FulfillmentTrackingUpdate($fulfillmentId: ID!, $trackingInfoInput: FulfillmentTrackingInput!, $notifyCustomer: Boolean) {
    fulfillmentTrackingInfoUpdate(fulfillmentId: $fulfillmentId, trackingInfoInput: $trackingInfoInput, notifyCustomer: $notifyCustomer) {
        fulfillment { ${FULFILLMENT} }
        ${USER_ERRORS}
    }
}`;

export const FULFILLMENT_ORDER_HOLD = `mutation FulfillmentOrderHold($id: ID!, $fulfillmentHold: FulfillmentOrderHoldInput!) {
    fulfillmentOrderHold(id: $id, fulfillmentHold: $fulfillmentHold) {
        fulfillmentHold { id reason }
        fulfillmentOrder { id status }
        ${CODED_USER_ERRORS}
    }
}`;

export const FULFILLMENT_ORDER_RELEASE = `mutation FulfillmentOrderRelease($id: ID!, $holdIds: [ID!]) {
    fulfillmentOrderReleaseHold(id: $id, holdIds: $holdIds) {
        fulfillmentOrder { id status }
        ${CODED_USER_ERRORS}
    }
}`;

export const SEARCH_PRODUCTS = `query SearchProducts($first: Int!, $after: String, $query: String) {
    products(first: $first, after: $after, query: $query) { nodes { ${PRODUCT_SUMMARY} } ${PAGE_INFO} }
}`;

export const GET_PRODUCT = `query GetProduct($id: ID!) {
    product(id: $id) {
        ${PRODUCT_SUMMARY} descriptionHtml
        variants(first: 100) { nodes { id title sku price compareAtPrice inventoryQuantity } pageInfo { hasNextPage } }
    }
}`;

export const PRODUCT_UPDATE = `mutation ProductUpdate($product: ProductUpdateInput!) {
    productUpdate(product: $product) { product { ${PRODUCT_SUMMARY} } ${USER_ERRORS} }
}`;

export const VARIANT_PRICES_UPDATE = `mutation VariantPricesUpdate($productId: ID!, $variants: [ProductVariantsBulkInput!]!) {
    productVariantsBulkUpdate(productId: $productId, variants: $variants) {
        productVariants { id title sku price compareAtPrice inventoryQuantity }
        ${CODED_USER_ERRORS}
    }
}`;

export const LIST_LOCATIONS = `query ListLocations {
    locations(first: 50) { nodes { id name isActive fulfillsOnlineOrders } }
}`;

export const INVENTORY_LEVELS = `query InventoryLevels($id: ID!, $first: Int!, $after: String) {
    product(id: $id) {
        id
        variants(first: $first, after: $after) {
            nodes {
                id title
                inventoryItem {
                    id
                    inventoryLevels(first: 10) {
                        nodes { location { id name } quantities(names: ["available", "committed", "on_hand"]) { name quantity } }
                    }
                }
            }
            ${PAGE_INFO}
        }
    }
}`;

export const AVAILABLE_AT = `query AvailableAt($id: ID!, $locationId: ID!) {
    inventoryItem(id: $id) { id inventoryLevel(locationId: $locationId) { quantities(names: ["available"]) { name quantity } } }
}`;

export const INVENTORY_SET = `mutation InventorySet($input: InventorySetQuantitiesInput!, $idempotencyKey: String!) {
    inventorySetQuantities(input: $input) @idempotent(key: $idempotencyKey) {
        inventoryAdjustmentGroup { id }
        ${CODED_USER_ERRORS}
    }
}`;

export const INVENTORY_ADJUST = `mutation InventoryAdjust($input: InventoryAdjustQuantitiesInput!, $idempotencyKey: String!) {
    inventoryAdjustQuantities(input: $input) @idempotent(key: $idempotencyKey) {
        inventoryAdjustmentGroup { id }
        ${CODED_USER_ERRORS}
    }
}`;

export const SEARCH_CUSTOMERS = `query SearchCustomers($first: Int!, $after: String, $query: String) {
    customers(first: $first, after: $after, query: $query) { nodes { ${CUSTOMER_SUMMARY} } ${PAGE_INFO} }
}`;

export const GET_CUSTOMER = `query GetCustomer($id: ID!) {
    customer(id: $id) {
        ${CUSTOMER_SUMMARY}
        amountSpent { amount currencyCode }
        orders(first: 10, sortKey: CREATED_AT, reverse: true) { nodes { ${ORDER_SUMMARY} } }
    }
}`;

export const LIST_ABANDONED_CHECKOUTS = `query ListAbandonedCheckouts($first: Int!, $after: String, $query: String) {
    abandonedCheckouts(first: $first, after: $after, query: $query, sortKey: CREATED_AT, reverse: true) {
        nodes { id name createdAt completedAt abandonedCheckoutUrl customer { defaultEmailAddress { emailAddress } } totalPriceSet { ${MONEY} } }
        ${PAGE_INFO}
    }
}`;

export const DISCOUNT_CREATE = `mutation DiscountCreate($basicCodeDiscount: DiscountCodeBasicInput!) {
    discountCodeBasicCreate(basicCodeDiscount: $basicCodeDiscount) { codeDiscountNode { id } ${CODED_USER_ERRORS} }
}`;

export const DISCOUNT_DEACTIVATE = `mutation DiscountDeactivate($id: ID!) {
    discountCodeDeactivate(id: $id) { codeDiscountNode { id } ${CODED_USER_ERRORS} }
}`;

export const DRAFT_ORDER_CREATE = `mutation DraftOrderCreate($input: DraftOrderInput!) {
    draftOrderCreate(input: $input) { draftOrder { ${DRAFT_ORDER} } ${USER_ERRORS} }
}`;

export const DRAFT_ORDER_SEND_INVOICE = `mutation DraftOrderSendInvoice($id: ID!, $email: EmailInput) {
    draftOrderInvoiceSend(id: $id, email: $email) { draftOrder { ${DRAFT_ORDER} } ${USER_ERRORS} }
}`;

export const GET_SHOP = `query GetShop {
    shop { name email currencyCode ianaTimezone myshopifyDomain plan { publicDisplayName } }
}`;
