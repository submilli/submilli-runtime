// Demo data only. These fictional fixtures mirror @submilli/chargeafter without
// HTTP or secret access. All money values use public currency units.
import { check } from "submilli:security";

/** A delivery channel for a generated link. */
export type DeliveryMethod = "Email" | "Sms" | "None";
/** ChargeAfter application channel. */
export type ApplicationChannel = "E_COMMERCE" | "IN_STORE" | "IN_HOME" | "CALL_CENTER";
/** A consumer who receives a link. */
export interface ConsumerInput { /** Given name. */ firstName: string; /** Family name. */ lastName: string; /** Email address. */ email?: string; /** Phone number. */ phone?: string; }
/** An item in a checkout cart. */
export interface CartItem { /** Merchant SKU. */ sku: string; /** Display name. */ name: string; /** Unit count. */ quantity: number; /** Unit price. */ unitPrice: number; }
/** Checkout totals and items. */
export interface CartInput { /** Purchased items. */ items: CartItem[]; /** Total amount. */ totalAmount: number; /** Tax amount. */ taxAmount?: number; /** Shipping amount. */ shippingAmount?: number; }
/** Inputs for an application link. */
export interface ApplyLinkInput { /** Link recipient. */ consumer: ConsumerInput; /** Delivery channel. */ deliveryMethod: DeliveryMethod; /** Provider link format. */ linkFormat?: string; /** Merchant store id. */ storeId: string; /** Application channel. */ channel: ApplicationChannel; }
/** Inputs for a checkout link. */
export interface CheckoutLinkInput extends ApplyLinkInput { /** Checkout cart. */ cart: CartInput; /** Whether to charge. */ charge?: boolean; /** Whether to settle. */ settle?: boolean; }
/** An existing Omni-Link. */
export interface LinkData { /** Current status. */ status: string; /** Link kind. */ linkType: string; /** Merchant store id. */ storeId: string | null; /** Creation time. */ createdAt: string | null; /** Update time. */ updatedAt: string | null; }
/** A newly generated link. */
export interface GeneratedLink { /** Link id. */ linkId: string; /** Destination URL. */ url: string | null; /** QR payload. */ qrCode: string | null; }
/** An application's current status. */
export interface ApplicationStatus { /** Application id. */ applicationId: string; /** Session state. */ sessionStatus: string; /** Flow type. */ flowType: string; /** Provider status. */ applicationStatus: string | null; /** Merchant store id. */ storeId: string | null; /** Lender id. */ lenderId: string | null; /** Lender name. */ lenderName: string | null; /** Available credit. */ availableCredit: number | null; }
/** A lender offer. */
export interface ApplicationOffer { /** Account id. */ accountId: string; /** Lender id. */ lenderId: string; /** Lender name. */ lenderName: string; /** Offer status. */ status: string; /** Offer amount. */ amount: number | null; /** Available credit. */ availableCredit: number | null; /** Promotion code. */ promoCode: string | null; }
/** A charge record. */
export interface Charge { /** Charge id. */ id: string; /** Current state. */ state: string; /** Authorized amount. */ authorizedAmount: number; /** Settled amount. */ settledAmount: number; /** Refunded amount. */ refundedAmount: number; /** Merchant order id. */ merchantOrderId: string; /** Merchant store id. */ storeId: string | null; }
/** A completed settlement, void, or refund. */
export interface ChargeTransaction { /** Transaction id. */ id: string; /** Charge id. */ chargeId: string; /** Amount. */ amount: number; /** Order id. */ merchantOrderId: string; /** Status. */ status: string; /** Creation time. */ createdAt: string; /** Update time. */ updatedAt: string; /** Lender id. */ lenderId: string; /** Merchant id. */ merchantId: string; /** Transaction type. */ type: string | null; }
/** Funding report filters. */
export interface FundingQuery { /** Inclusive start date. */ fromDate: string; /** Inclusive end date. */ toDate: string; /** Page offset. */ offset?: number; /** Page size. */ limit?: number; /** Include missing funding. */ showMissing?: boolean; }
/** A funding transaction. */
export interface FundingRecord { /** Charge id. */ chargeId: string | null; /** Transaction id. */ transactionId: string | null; /** Transaction type. */ transactionType: string; /** Transaction amount. */ transactionAmount: number; /** Merchant order id. */ merchantOrderId: string; /** Merchant transaction id. */ merchantTransactionId: string | null; /** Funding status. */ status: string; /** Funding time. */ fundedAt: string | null; /** Lender name. */ lenderName: string | null; /** Net amount. */ netAmount: number | null; }
/** A funding result page. */
export interface FundingPage { /** Funding records. */ records: FundingRecord[]; /** Page offset. */ offset: number; /** Page size. */ limit: number; }
/** Consumer lookup selectors. */
/** Postal address used by the consumer lookup. */
export interface Address { /** State. */ state: string; /** City. */ city: string; /** First address line. */ line1: string; /** Postal code. */ zipCode: string; /** Second address line. */ line2?: string; }
/** Full consumer details required for the API lookup. */
export interface ConsumerLookup { /** Given name. */ firstName: string; /** Family name. */ lastName: string; /** Email address. */ email: string; /** Shipping address. */ shippingAddress: Address; /** Billing address. */ billingAddress: Address; /** Net monthly income. */ netMonthlyIncome: number; /** Date of birth. */ dateOfBirth: string; /** Phone number. */ phone?: string; }
/** A consumer account. */
export interface ConsumerAccount { /** Account id. */ id: string; /** Lender id. */ lenderId: string; /** Lender name. */ lenderName: string; /** Account status. */ status: string; /** Available credit. */ availableCredit: number | null; }
/** Consumer lookup result. */
export interface ConsumerLookupResult { /** Application id. */ applicationId: string; /** Related accounts. */ accounts: ConsumerAccount[]; }

interface StoredApplication { value: ApplicationStatus; offers: ApplicationOffer[]; }
interface StoredCharge { value: Charge; voidedAmount: number; }
interface StoredConsumer { firstName: string; lastName: string; email: string; phone: string; result: ConsumerLookupResult; }
const stores = ["pied_piper", "hooli", "tres_comas"];
const applications: StoredApplication[] = [
    app("application-created", "Created", null), app("application-link-sent", "LinkSent", null), app("application-prequalified", "Prequalified", "account-ada"),
    app("application-offer-selected", "OfferSelected", "account-ben"), app("application-approved", "Approved", "account-cam"), app("application-declined", "Declined", null),
    app("application-expired", "Expired", null), app("application-cancelled", "Cancelled", null), app("application-charged", "Charged", "account-ada"),
    app("application-settled", "Settled", "account-ben"), app("application-refunded", "Refunded", "account-cam"), app("application-disputed", "Disputed", "account-dev"),
];
const links: LinkData[] = [
    { status: "Active", linkType: "Application", storeId: "pied_piper", createdAt: "2026-10-01T12:00:00Z", updatedAt: "2026-10-01T12:00:00Z" },
    { status: "Used", linkType: "Checkout", storeId: "hooli", createdAt: "2026-10-02T12:00:00Z", updatedAt: "2026-10-03T12:00:00Z" },
];
const generatedLinks = new Map<string, LinkData>();
const charges: StoredCharge[] = [
    storedCharge("charge-authorized", "Authorized", 2400, 0, 0, "order-pied-1001", 0),
    storedCharge("charge-partial-settlement", "PartiallySettled", 1800, 900, 0, "order-hooli-1002", 0),
    storedCharge("charge-settled", "Settled", 3200, 3200, 0, "order-tres-1003", 0),
    storedCharge("charge-voided", "Voided", 1100, 0, 0, "order-pied-1004", 1100),
    storedCharge("charge-partial-refund", "PartiallyRefunded", 2600, 2600, 650, "order-hooli-1005", 0),
    storedCharge("charge-refunded", "Refunded", 1500, 1500, 1500, "order-tres-1006", 0),
    storedCharge("charge-disputed", "Disputed", 4000, 4000, 0, "order-pied-1007", 0),
    storedCharge("charge-decimal", "Authorized", 0.3, 0, 0, "order-pied-1008", 0),
    storedCharge("charge-decimal-void", "Authorized", 0.3, 0, 0, "order-pied-1009", 0),
];
const fundings: FundingRecord[] = [
    funding("charge-partial-settlement", "tx-hooli-1002", 900, "order-hooli-1002", "Funded", "2026-10-03", "Fixture Finance", 846),
    funding("charge-settled", "tx-tres-1003", 3200, "order-tres-1003", "Funded", "2026-10-04", "Demo Credit", 3008),
    funding("charge-disputed", "tx-pied-1007", 4000, "order-pied-1007", "Missing", null, "Demo Credit", null),
];
const consumers: StoredConsumer[] = [
    consumer("consumer-ada", "Ada", "Morris", "ada@example.test", "+15550000001", "account-ada", "Open", 800),
    consumer("consumer-ben", "Ben", "Ortiz", "ben@example.test", "+15550000002", "account-ben", "Closed", 0),
    consumer("consumer-cam", "Cam", "Singh", "cam@example.test", "+15550000003", "account-cam", "Open", 1200),
    consumer("consumer-dev", "Dev", "Park", "dev@example.test", "+15550000004", "account-dev", "Delinquent", null),
];
let nextLink = 1;

/**
 * Get a fictional Omni-Link.
 * @param linkId Link id.
 * @returns Link metadata or null.
 * @capability chargeafter.com/links.get { linkId: string }
 */
export function getLink(linkId: string): LinkData | null { const id = requiredText(linkId, "linkId"); check("chargeafter.com/links.get", { linkId: id }); return copyLink(findLink(id)); }

/**
 * Create a fictional application link.
 * @param input Link input.
 * @returns Generated link.
 * @capability chargeafter.com/links.sendApply { storeId: string, deliveryMethod: string }
 */
export function sendApplyLink(input: ApplyLinkInput): GeneratedLink {
    const consumer = input.consumer;
    const prepared = prepareLink(consumer.firstName, consumer.lastName, consumer.email, consumer.phone, input.deliveryMethod, input.linkFormat, input.storeId);
    check("chargeafter.com/links.sendApply", { storeId: prepared.storeId, deliveryMethod: prepared.deliveryMethod });
    return newLink(prepared, "Apply");
}

/**
 * Create a fictional checkout link.
 * @param input Link input.
 * @returns Generated link.
 * @capability chargeafter.com/links.sendCheckout { storeId: string, deliveryMethod: string, totalAmount: number, autoCharge: boolean }
 */
export function sendCheckoutLink(input: CheckoutLinkInput): GeneratedLink {
    const consumer = input.consumer;
    const prepared = prepareLink(consumer.firstName, consumer.lastName, consumer.email, consumer.phone, input.deliveryMethod, input.linkFormat, input.storeId);
    const cartInput = input.cart;
    const itemSnapshots: CartItem[] = [];
    for (const supplied of cartInput.items) { const sku = supplied.sku; const name = supplied.name; const quantity = supplied.quantity; const unitPrice = supplied.unitPrice; itemSnapshots.push({ sku: sku, name: name, quantity: quantity, unitPrice: unitPrice }); }
    const totalAmount = cartAmount(itemSnapshots, cartInput.totalAmount, cartInput.taxAmount, cartInput.shippingAmount); const autoCharge = input.charge === true; const settle = input.settle === true;
    if (settle && !autoCharge) throw new Error("settle requires charge");
    check("chargeafter.com/links.sendCheckout", { storeId: prepared.storeId, deliveryMethod: prepared.deliveryMethod, totalAmount: totalAmount, autoCharge: autoCharge });
    return newLink(prepared, "Checkout");
}

/**
 * Get a fictional application status.
 * @param applicationId Application id.
 * @returns Status or null.
 * @capability chargeafter.com/applications.status { applicationId: string }
 */
export function getApplicationStatus(applicationId: string): ApplicationStatus | null { const id = requiredText(applicationId, "applicationId"); check("chargeafter.com/applications.status", { applicationId: id }); const found = findApplication(id); return found === undefined ? null : copyStatus(found.value); }

/**
 * List fictional lender offers.
 * @param applicationId Application id.
 * @returns Offers.
 * @capability chargeafter.com/applications.offers { applicationId: string }
 */
export function listApplicationOffers(applicationId: string): ApplicationOffer[] { const id = requiredText(applicationId, "applicationId"); check("chargeafter.com/applications.offers", { applicationId: id }); const found = findApplication(id); return found === undefined ? [] : copyOffers(found.offers); }

/**
 * Get a fictional charge.
 * @param chargeId Charge id.
 * @returns Charge or null.
 * @capability chargeafter.com/charges.get { chargeId: string }
 */
export function getCharge(chargeId: string): Charge | null { const id = requiredText(chargeId, "chargeId"); check("chargeafter.com/charges.get", { chargeId: id }); const found = findCharge(id); return found === undefined ? null : copyCharge(found.value); }

/**
 * Settle a fictional charge.
 * @param chargeId Charge id.
 * @param amountValue Currency amount.
 * @param merchantTransactionId Transaction id.
 * @returns Transaction.
 * @capability chargeafter.com/charges.settle { chargeId: string, amount: number, authorizedAmount: number, merchantOrderId: string }
 */
export function settleCharge(chargeId: string, amountValue?: number, merchantTransactionId?: string): ChargeTransaction {
    const id = requiredText(chargeId, "chargeId"); const requested = amountValue === undefined ? undefined : cents(amountValue, "amount"); optionalText(merchantTransactionId, "merchantTransactionId"); const stored = requireCharge(id);
    const remaining = roundMoney(stored.value.authorizedAmount - stored.value.settledAmount - stored.voidedAmount);
    const value = requested ?? remaining;
    if (value <= 0 || value > remaining) throw new Error("amount exceeds the remaining authorized amount");
    check("chargeafter.com/charges.settle", { chargeId: id, amount: value, authorizedAmount: stored.value.authorizedAmount, merchantOrderId: stored.value.merchantOrderId });
    stored.value.settledAmount = roundMoney(stored.value.settledAmount + value); stored.value.state = stored.value.settledAmount === stored.value.authorizedAmount ? "Settled" : "PartiallySettled"; return transaction(stored.value, value, "Settled");
}

/**
 * Void a fictional charge.
 * @param chargeId Charge id.
 * @param amountValue Currency amount.
 * @param merchantTransactionId Transaction id.
 * @returns Transaction.
 * @capability chargeafter.com/charges.void { chargeId: string, amount: number, state: string }
 */
export function voidCharge(chargeId: string, amountValue?: number, merchantTransactionId?: string): ChargeTransaction {
    const id = requiredText(chargeId, "chargeId"); const requested = amountValue === undefined ? undefined : cents(amountValue, "amount"); optionalText(merchantTransactionId, "merchantTransactionId"); const stored = requireCharge(id);
    const remaining = roundMoney(stored.value.authorizedAmount - stored.value.settledAmount - stored.voidedAmount);
    const value = requested ?? remaining;
    if (value <= 0 || value > remaining) throw new Error("amount exceeds the remaining authorized amount");
    check("chargeafter.com/charges.void", { chargeId: id, amount: value, state: stored.value.state });
    stored.voidedAmount = roundMoney(stored.voidedAmount + value); stored.value.state = stored.voidedAmount === stored.value.authorizedAmount ? "Voided" : "PartiallySettled"; return transaction(stored.value, value, "Voided");
}

/**
 * Refund a fictional charge.
 * @param chargeId Charge id.
 * @param amountValue Currency amount.
 * @param reason Reason.
 * @param merchantTransactionId Transaction id.
 * @returns Transaction.
 * @capability chargeafter.com/charges.refund { chargeId: string, amount: number, settledAmount: number }
 */
export function refundCharge(chargeId: string, amountValue: number, reason?: string, merchantTransactionId?: string): ChargeTransaction {
    const id = requiredText(chargeId, "chargeId"); const value = cents(amountValue, "amount"); if (reason !== undefined) throw new Error("refund reason is not supported by ChargeAfter"); optionalText(merchantTransactionId, "merchantTransactionId"); const stored = requireCharge(id);
    if (value > roundMoney(stored.value.settledAmount - stored.value.refundedAmount)) throw new Error("amount exceeds the remaining refundable amount");
    check("chargeafter.com/charges.refund", { chargeId: id, amount: value, settledAmount: stored.value.settledAmount });
    stored.value.refundedAmount = roundMoney(stored.value.refundedAmount + value); stored.value.state = stored.value.refundedAmount === stored.value.settledAmount ? "Refunded" : "PartiallyRefunded"; return transaction(stored.value, value, "Refunded");
}

/**
 * Update a fictional merchant order id.
 * @param chargeId Charge id.
 * @param merchantOrderId Order id.
 * @returns Charge.
 * @capability chargeafter.com/charges.update { chargeId: string }
 */
export function setChargeOrderId(chargeId: string, merchantOrderId: string): Charge { const id = requiredText(chargeId, "chargeId"); const order = requiredText(merchantOrderId, "merchantOrderId"); if (order.length > 256) throw new Error("merchantOrderId must be at most 256 characters"); check("chargeafter.com/charges.update", { chargeId: id }); const stored = requireCharge(id); stored.value.merchantOrderId = order; return copyCharge(stored.value); }

/**
 * List the fictional funding report.
 * @param input Report query.
 * @returns Funding page.
 * @capability chargeafter.com/fundings.list { fromDate: string, toDate: string }
 */
export function listFundings(input: FundingQuery): FundingPage {
    const fromDate = date(input.fromDate, "fromDate"); const toDate = date(input.toDate, "toDate"); const offset = input.offset ?? 0; const limit = input.limit ?? 100; page(offset, limit);
    check("chargeafter.com/fundings.list", { fromDate: fromDate, toDate: toDate }); const records: FundingRecord[] = [];
    for (const record of fundings) if ((record.fundedAt === null || (record.fundedAt >= fromDate && record.fundedAt <= toDate)) && (input.showMissing === true || record.status !== "Missing")) records.push(copyFunding(record));
    const selected: FundingRecord[] = []; for (let index = offset; index < records.length && selected.length < limit; index += 1) selected.push(records[index]); return { records: selected, offset: offset, limit: limit };
}

/**
 * Look up a fictional consumer.
 * @param input Selectors.
 * @returns Result or null.
 * @capability chargeafter.com/consumers.lookup { email: string, phone: string }
 */
export function lookupConsumer(input: ConsumerLookup): ConsumerLookupResult | null {
    const firstName = requiredText(input.firstName, "firstName");
    const lastName = requiredText(input.lastName, "lastName");
    const email = requiredText(input.email, "email");
    validateAddress(input.shippingAddress, "shippingAddress");
    validateAddress(input.billingAddress, "billingAddress");
    if (!Number.isFinite(input.netMonthlyIncome)) throw new Error("netMonthlyIncome must be finite");
    date(input.dateOfBirth, "dateOfBirth");
    const phone = optionalText(input.phone, "phone");
    check("chargeafter.com/consumers.lookup", { email: email, phone: phone ?? "" });
    for (const stored of consumers) if (stored.email === email && stored.phone === (phone ?? stored.phone) && stored.firstName === firstName && stored.lastName === lastName) return copyLookup(stored.result);
    return null;
}

/**
 * List fictional consumer accounts.
 * @param consumerId Consumer id.
 * @returns Accounts.
 * @capability chargeafter.com/consumers.accounts { consumerId: string }
 */
export function getConsumerAccounts(consumerId: string): ConsumerAccount[] { const id = requiredText(consumerId, "consumerId"); check("chargeafter.com/consumers.accounts", { consumerId: id }); for (const stored of consumers) if (stored.result.applicationId === id) return copyAccounts(stored.result.accounts); return []; }

function app(applicationId: string, applicationStatus: string, accountId: string | null): StoredApplication { const offer: ApplicationOffer = { accountId: accountId ?? "", lenderId: accountId === null ? "" : "lender-demo", lenderName: accountId === null ? "" : "Demo Finance", status: accountId === null ? "Unavailable" : "Available", amount: accountId === null ? null : 2500, availableCredit: accountId === null ? null : 2500, promoCode: null }; return { value: { applicationId: applicationId, sessionStatus: "Complete", flowType: "Application", applicationStatus: applicationStatus, storeId: accountId === null ? null : "pied_piper", lenderId: accountId === null ? null : "lender-demo", lenderName: accountId === null ? null : "Demo Finance", availableCredit: accountId === null ? null : 2500 }, offers: [offer] }; }
function storedCharge(id: string, state: string, authorizedAmount: number, settledAmount: number, refundedAmount: number, merchantOrderId: string, voidedAmount: number): StoredCharge { return { value: { id: id, state: state, authorizedAmount: authorizedAmount, settledAmount: settledAmount, refundedAmount: refundedAmount, merchantOrderId: merchantOrderId, storeId: merchantOrderId.includes("hooli") ? "hooli" : merchantOrderId.includes("tres") ? "tres_comas" : "pied_piper" }, voidedAmount: voidedAmount }; }
function funding(chargeId: string | null, transactionId: string | null, transactionAmount: number, merchantOrderId: string, status: string, fundedAt: string | null, lenderName: string | null, netAmount: number | null): FundingRecord { return { chargeId: chargeId, transactionId: transactionId, transactionType: "Settlement", transactionAmount: transactionAmount, merchantOrderId: merchantOrderId, merchantTransactionId: null, status: status, fundedAt: fundedAt, lenderName: lenderName, netAmount: netAmount }; }
function consumer(applicationId: string, firstName: string, lastName: string, email: string, phone: string, accountId: string, status: string, availableCredit: number | null): StoredConsumer { return { firstName: firstName, lastName: lastName, email: email, phone: phone, result: { applicationId: applicationId, accounts: [{ id: accountId, lenderId: "lender-demo", lenderName: "Demo Finance", status: status, availableCredit: availableCredit }] } }; }
function prepareLink(first: string, last: string, rawEmail: string | undefined, rawPhone: string | undefined, rawMethod: DeliveryMethod, rawFormat: string | undefined, rawStoreId: string): { storeId: string; deliveryMethod: DeliveryMethod } {
    const storeId = requiredText(rawStoreId, "storeId");
    let known = false;
    for (const store of stores) if (store === storeId) known = true;
    if (!known) throw new Error("storeId is not a fictional contractor");
    if (rawMethod !== "Email" && rawMethod !== "Sms" && rawMethod !== "None") throw new Error("deliveryMethod must be Email, Sms, or None");
    requiredText(first, "consumer.firstName"); requiredText(last, "consumer.lastName");
    if (optionalText(rawEmail, "consumer.email") === undefined && optionalText(rawPhone, "consumer.phone") === undefined) throw new Error("consumer.email or consumer.phone is required");
    optionalText(rawFormat, "linkFormat");
    return { storeId: storeId, deliveryMethod: rawMethod };
}
function cartAmount(items: CartItem[], rawTotal: number, rawTax: number | undefined, rawShipping: number | undefined): number {
    const total = cents(rawTotal, "cart.totalAmount");
    if (items.length === 0) throw new Error("cart.items must not be empty");
    for (const item of items) { requiredText(item.sku, "item.sku"); requiredText(item.name, "item.name"); if (!Number.isInteger(item.quantity) || item.quantity < 1) throw new Error("item.quantity must be a positive integer"); cents(item.unitPrice, "item.unitPrice"); }
    if (rawTax !== undefined) zeroOrCents(rawTax, "cart.taxAmount");
    if (rawShipping !== undefined) zeroOrCents(rawShipping, "cart.shippingAmount");
    return total;
}
function newLink(input: { storeId: string; deliveryMethod: DeliveryMethod }, linkType: string): GeneratedLink {
    const id = "link-demo-" + nextLink.toString();
    nextLink += 1;
    generatedLinks.set(id, { status: "Active", linkType: linkType, storeId: input.storeId, createdAt: "2026-10-06T12:00:00Z", updatedAt: "2026-10-06T12:00:00Z" });
    return { linkId: id, url: "https://demo.invalid/" + input.storeId + "/" + linkType.toLowerCase() + "/" + id, qrCode: "demo-qr-" + id };
}
function findLink(id: string): LinkData | null { if (id === "link-apply-ada") return links[0]; if (id === "link-checkout-ben") return links[1]; return generatedLinks.get(id) ?? null; }
function findApplication(id: string): StoredApplication | undefined { for (const app of applications) if (app.value.applicationId === id) return app; return undefined; }
function findCharge(id: string): StoredCharge | undefined { for (const charge of charges) if (charge.value.id === id) return charge; return undefined; }
function requireCharge(id: string): StoredCharge { const found = findCharge(id); if (found === undefined) throw new Error("charge not found"); return found; }
function requiredText(value: string, name: string): string { const result = value.trim(); if (result.length === 0) throw new Error(name + " must not be empty"); return result; }
function optionalText(value: string | undefined, name: string): string | undefined { return value === undefined ? undefined : requiredText(value, name); }
function cents(value: number, name: string): number {
    if (!Number.isFinite(value) || value <= 0 || Math.abs(value * 100 - Math.round(value * 100)) > 0.0000001) throw new Error(name + " must be a positive currency amount with at most two decimals");
    return Math.round(value * 100) / 100;
}
function zeroOrCents(value: number, name: string): number {
    if (!Number.isFinite(value) || value < 0 || Math.abs(value * 100 - Math.round(value * 100)) > 0.0000001) throw new Error(name + " must be a currency amount with at most two decimals");
    return Math.round(value * 100) / 100;
}
function roundMoney(value: number): number { return Math.round(value * 100) / 100; }
function validateAddress(value: Address, name: string): void { requiredText(value.state, name + ".state"); requiredText(value.city, name + ".city"); requiredText(value.line1, name + ".line1"); requiredText(value.zipCode, name + ".zipCode"); if (value.line2 !== undefined) optionalText(value.line2, name + ".line2"); }
function date(value: string, name: string): string { const result = requiredText(value, name); if (result.length !== 10 || result.charAt(4) !== "-" || result.charAt(7) !== "-") throw new Error(name + " must be YYYY-MM-DD"); return result; }
function page(offset: number, limit: number): void { if (!Number.isInteger(offset) || offset < 0) throw new Error("offset must be a non-negative integer"); if (!Number.isInteger(limit) || limit < 1 || limit > 1000) throw new Error("limit must be an integer from 1 through 1000"); }
function copyLink(value: LinkData | null): LinkData | null { return value === null ? null : { status: value.status, linkType: value.linkType, storeId: value.storeId, createdAt: value.createdAt, updatedAt: value.updatedAt }; }
function copyStatus(value: ApplicationStatus): ApplicationStatus { return { applicationId: value.applicationId, sessionStatus: value.sessionStatus, flowType: value.flowType, applicationStatus: value.applicationStatus, storeId: value.storeId, lenderId: value.lenderId, lenderName: value.lenderName, availableCredit: value.availableCredit }; }
function copyOffers(values: ApplicationOffer[]): ApplicationOffer[] { const copies: ApplicationOffer[] = []; for (const value of values) copies.push({ accountId: value.accountId, lenderId: value.lenderId, lenderName: value.lenderName, status: value.status, amount: value.amount, availableCredit: value.availableCredit, promoCode: value.promoCode }); return copies; }
function copyCharge(value: Charge): Charge { return { id: value.id, state: value.state, authorizedAmount: value.authorizedAmount, settledAmount: value.settledAmount, refundedAmount: value.refundedAmount, merchantOrderId: value.merchantOrderId, storeId: value.storeId }; }
function transaction(charge: Charge, amount: number, status: string): ChargeTransaction { return { id: "transaction-" + charge.id + "-" + status, chargeId: charge.id, amount: amount, merchantOrderId: charge.merchantOrderId, status: status, createdAt: "2026-10-06T12:00:00Z", updatedAt: "2026-10-06T12:00:00Z", lenderId: "lender-demo", merchantId: "merchant-demo", type: status }; }
function copyFunding(value: FundingRecord): FundingRecord { return { chargeId: value.chargeId, transactionId: value.transactionId, transactionType: value.transactionType, transactionAmount: value.transactionAmount, merchantOrderId: value.merchantOrderId, merchantTransactionId: value.merchantTransactionId, status: value.status, fundedAt: value.fundedAt, lenderName: value.lenderName, netAmount: value.netAmount }; }
function copyAccounts(values: ConsumerAccount[]): ConsumerAccount[] { const copies: ConsumerAccount[] = []; for (const value of values) copies.push({ id: value.id, lenderId: value.lenderId, lenderName: value.lenderName, status: value.status, availableCredit: value.availableCredit }); return copies; }
function copyLookup(value: ConsumerLookupResult): ConsumerLookupResult { return { applicationId: value.applicationId, accounts: copyAccounts(value.accounts) }; }
