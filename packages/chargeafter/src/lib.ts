import { get, patch, post, Response } from "submilli:http";
import secrets from "submilli:secrets";
import { check } from "submilli:security";
import { baseUrl } from "./config";
import { ChargeAfterError, invalidArgument } from "./errors";
import { amount, date, optionalText, page, requiredText } from "./request";
import { consumerAccountsResponse, ConsumerAccountInfoWire, ConsumerAccountWire, nullable, requireOk } from "./response";

export { ChargeAfterError } from "./errors";

/** How ChargeAfter delivers a generated link. */
export type DeliveryMethod = "Email" | "Sms" | "None";
/** ChargeAfter application channel. */
export type ApplicationChannel = "E_COMMERCE" | "IN_STORE" | "IN_HOME" | "CALL_CENTER";

/** A consumer to receive a link. */
export interface ConsumerInput {
    /** Given name. */
    firstName: string;
    /** Family name. */
    lastName: string;
    /** Optional email address. */
    email?: string;
    /** Optional phone number. */
    phone?: string;
}

/** One purchasable cart item. */
export interface CartItem {
    /** Merchant SKU. */
    sku: string;
    /** Display name. */
    name: string;
    /** Positive item count. */
    quantity: number;
    /** Price per item. */
    unitPrice: number;
}

/** Checkout cart totals. */
export interface CartInput {
    /** Items in the cart. */
    items: CartItem[];
    /** Full cart total. */
    totalAmount: number;
    /** Optional tax component. */
    taxAmount?: number;
    /** Optional shipping component. */
    shippingAmount?: number;
}

/** Inputs shared by Apply and Checkout links. */
export interface ApplyLinkInput {
    /** Consumer receiving the link. */
    consumer: ConsumerInput;
    /** Delivery channel. */
    deliveryMethod: DeliveryMethod;
    /** Optional ChargeAfter link format. */
    linkFormat?: string;
    /** Merchant store identifier. */
    storeId: string;
    /** ChargeAfter application channel. */
    channel: ApplicationChannel;
}

/** Inputs for a Checkout link. */
export interface CheckoutLinkInput extends ApplyLinkInput {
    /** Cart to finance. */
    cart: CartInput;
    /** Whether ChargeAfter should charge automatically. */
    charge?: boolean;
    /** Whether ChargeAfter should settle automatically. */
    settle?: boolean;
}

/** Link returned when ChargeAfter creates an Omni-Link. */
export interface GeneratedLink {
    /** Generated link identifier. */
    linkId: string;
    /** Hosted URL, when available. */
    url: string | null;
    /** Base64 QR code, when available. */
    qrCode: string | null;
}

/** The data ChargeAfter returns for an existing Omni-Link. It does not include its URL or id. */
export interface LinkData {
    /** Current link status. */
    status: string;
    /** Link kind. */
    linkType: string;
    /** Creation timestamp. */
    createdAt: string | null;
    /** Last-update timestamp. */
    updatedAt: string | null;
    /** Merchant store identifier when returned by the provider. */
    storeId: string | null;
}

/** Current application status and selected account details. */
export interface ApplicationStatus {
    /** Application identifier. */
    applicationId: string;
    /** ChargeAfter session state. */
    sessionStatus: string;
    /** ChargeAfter flow kind. */
    flowType: string;
    /** ChargeAfter application state, when assigned. */
    applicationStatus: string | null;
    /** Selected lender, if any. */
    lenderId: string | null;
    /** Selected lender name, if any. */
    lenderName: string | null;
    /** Available credit, if supplied. */
    availableCredit: number | null;
    /** Merchant store identifier when returned by the provider. */
    storeId: string | null;
}

/** A lender offer flattened from an account. */
export interface ApplicationOffer {
    /** Account identifier. */
    accountId: string;
    /** Lender identifier. */
    lenderId: string;
    /** Lender name. */
    lenderName: string;
    /** Account status. */
    status: string;
    /** Offered amount. */
    amount: number | null;
    /** Available credit when supplied. */
    availableCredit: number | null;
    /** Promotional offer code. */
    promoCode: string | null;
}

/** Amounts are returned by ChargeAfter and are never copied from caller input. */
export interface Charge {
    /** Charge identifier. */
    id: string;
    /** Charge lifecycle state. */
    state: string;
    /** `totalAmount` from ChargeAfter; normalized as the policy's authorizedAmount. */
    authorizedAmount: number;
    /** Amount already settled. */
    settledAmount: number;
    /** Amount already refunded. */
    refundedAmount: number;
    /** Merchant order identifier. */
    merchantOrderId: string;
    /** Merchant store identifier when returned by the provider. */
    storeId: string | null;
}

/** A completed ChargeAfter settle, void, or refund transaction. */
export interface ChargeTransaction {
    /** Transaction identifier. */
    id: string;
    /** Related charge identifier. */
    chargeId: string;
    /** Transaction amount. */
    amount: number;
    /** Merchant order identifier. */
    merchantOrderId: string;
    /** Provider transaction status. */
    status: string;
    /** Creation timestamp. */
    createdAt: string;
    /** Update timestamp. */
    updatedAt: string;
    /** Lender identifier. */
    lenderId: string;
    /** Merchant identifier. */
    merchantId: string;
    /** Provider transaction type, when supplied. */
    type: string | null;
}

/** Funding-report date range and bounded page. */
export interface FundingQuery {
    /** Inclusive report start date. */
    fromDate: string;
    /** Inclusive report end date. */
    toDate: string;
    /** Zero-based page offset. */
    offset?: number;
    /** Page size. */
    limit?: number;
    /** Include missing reconciliation rows. */
    showMissing?: boolean;
}

/** One lender funding-reconciliation row. */
export interface FundingRecord {
    /** Charge identifier. */
    chargeId: string | null;
    /** Lender transaction identifier. */
    transactionId: string | null;
    /** Transaction kind. */
    transactionType: string;
    /** Transaction amount. */
    transactionAmount: number;
    /** Merchant order identifier. */
    merchantOrderId: string;
    /** Merchant transaction identifier. */
    merchantTransactionId: string | null;
    /** Reconciliation status. */
    status: string;
    /** Funding timestamp. */
    fundedAt: string | null;
    /** Lender name. */
    lenderName: string | null;
    /** Merchant net amount. */
    netAmount: number | null;
}

/** A bounded funding-report page. */
export interface FundingPage {
    /** Returned reconciliation rows. */
    records: FundingRecord[];
    /** Requested offset. */
    offset: number;
    /** Requested limit. */
    limit: number;
}

/** A consumer street address required by ChargeAfter's account lookup. */
export interface Address { /** State. */ state: string; /** City. */ city: string; /** Address line. */ line1: string; /** Postal code. */ zipCode: string; /** Optional second line. */ line2?: string; }
/** Consumer details required by ChargeAfter's account lookup. It has no email-only endpoint. */
export interface ConsumerLookup { /** First name. */ firstName: string; /** Last name. */ lastName: string; /** Email. */ email: string; /** Shipping address. */ shippingAddress: Address; /** Billing address. */ billingAddress: Address; /** Net monthly income. */ netMonthlyIncome: number; /** Date of birth. */ dateOfBirth: string; /** Mobile phone. */ phone?: string; }
/** Result of ChargeAfter's contact-based account lookup. */
export interface ConsumerLookupResult {
    /** Application created or found by the lookup. */
    applicationId: string;
    /** Matching lender accounts. */
    accounts: ConsumerAccount[];
}
/** One lender account associated with a consumer. */
export interface ConsumerAccount {
    /** Account identifier. */
    id: string;
    /** Lender identifier. */
    lenderId: string;
    /** Lender name. */
    lenderName: string;
    /** Account status. */
    status: string;
    /** Available credit, when supplied. */
    availableCredit: number | null;
}

/** Get an Omni-Link. Returns null only when ChargeAfter reports that the link is absent. */
/** Get one existing Omni-Link.
 * @param linkId Link identifier.
 * @returns Link data, or null when absent.
 * @capability chargeafter.com/links.get { linkId: string }
 */
export function getLink(linkId: string): LinkData | null {
    const id = requiredText(linkId, "linkId");
    check("chargeafter.com/links.get", { linkId: id });
    const response = nullable(caGetRaw("/v2/omni-link/consumer/" + encodeURIComponent(id)));
    return response === null ? null : linkDataFrom(response.json() as LinkDataWire);
}

/** Send an Apply link.
 * @param input Consumer and delivery details.
 * @returns The generated link.
 * @capability chargeafter.com/links.sendApply { storeId: string, deliveryMethod: string }
 */
export function sendApplyLink(input: ApplyLinkInput): GeneratedLink {
    const consumer = input.consumer;
    const storeId = requiredText(input.storeId, "storeId"); const deliveryMethod = input.deliveryMethod;
    const prepared = prepareLink(consumer.firstName, consumer.lastName, consumer.email, consumer.phone, deliveryMethod, input.linkFormat, storeId, input.channel);
    check("chargeafter.com/links.sendApply", { storeId: storeId, deliveryMethod: deliveryMethod });
    return generatedLinkFrom(caPost("/v2/omni-link/consumer/application", prepared).json() as GeneratedLinkWire);
}

/** Send a Checkout link.
 * @param input Consumer, cart, and delivery details.
 * @returns The generated link.
 * @capability chargeafter.com/links.sendCheckout { storeId: string, deliveryMethod: string, totalAmount: number, autoCharge: boolean }
 */
export function sendCheckoutLink(input: CheckoutLinkInput): GeneratedLink {
    const consumer = input.consumer;
    const storeId = requiredText(input.storeId, "storeId"); const deliveryMethod = input.deliveryMethod;
    const prepared = prepareLink(consumer.firstName, consumer.lastName, consumer.email, consumer.phone, deliveryMethod, input.linkFormat, storeId, input.channel);
    const cartInput = input.cart;
    const itemSnapshots: CartItem[] = [];
    for (const supplied of cartInput.items) {
        const sku = supplied.sku;
        const name = supplied.name;
        const quantity = supplied.quantity;
        const unitPrice = supplied.unitPrice;
        itemSnapshots.push({ sku: sku, name: name, quantity: quantity, unitPrice: unitPrice });
    }
    const cart = prepareCart(itemSnapshots, cartInput.totalAmount, cartInput.taxAmount, cartInput.shippingAmount);
    const autoCharge = input.charge === true;
    const settle = input.settle === true;
    check("chargeafter.com/links.sendCheckout", { storeId: storeId, deliveryMethod: deliveryMethod, totalAmount: cart.totalAmount, autoCharge: autoCharge });
    return generatedLinkFrom(caPost("/v2/omni-link/consumer/checkout", { ...prepared, cart: cart, checkoutOptions: { charge: autoCharge, settle: settle } }).json() as GeneratedLinkWire);
}

/** Get an application status.
 * @param applicationId Application identifier.
 * @returns The status, or null when absent.
 * @capability chargeafter.com/applications.status { applicationId: string }
 */
export function getApplicationStatus(applicationId: string): ApplicationStatus | null {
    const id = requiredText(applicationId, "applicationId");
    check("chargeafter.com/applications.status", { applicationId: id });
    const response = nullable(caGetRaw("/v2/checkout/applications/" + encodeURIComponent(id) + "/status"));
    return response === null ? null : applicationStatusFrom(id, response.json() as ApplicationStatusWire);
}

/** List lender offers for an application.
 * @param applicationId Application identifier.
 * @returns Flattened offers.
 * @capability chargeafter.com/applications.offers { applicationId: string }
 */
export function listApplicationOffers(applicationId: string): ApplicationOffer[] {
    const id = requiredText(applicationId, "applicationId");
    check("chargeafter.com/applications.offers", { applicationId: id });
    const data = caPost("/v2/checkout/applications/" + encodeURIComponent(id) + "/accounts", {}).json() as { accountsInfo?: ApplicationOfferWire[] | null };
    const accounts = data.accountsInfo ?? [];
    const result: ApplicationOffer[] = [];
    for (const account of accounts) {
        for (const offer of account.offers ?? []) result.push(applicationOfferFrom(account, offer));
    }
    return result;
}

/** Get a charge.
 * @param chargeId Charge identifier.
 * @returns The charge, or null when absent.
 * @capability chargeafter.com/charges.get { chargeId: string }
 */
export function getCharge(chargeId: string): Charge | null {
    const id = requiredText(chargeId, "chargeId");
    check("chargeafter.com/charges.get", { chargeId: id });
    return fetchCharge(id);
}

/** Reads the charge before authorizing the write, so the caller cannot inflate its authorized amount. */
/** Settle a charge after reading its authoritative amount.
 * @param chargeId Charge identifier.
 * @param amountValue Optional settlement amount.
 * @param merchantTransactionId Optional merchant idempotency reference.
 * @returns The created settlement transaction.
 * @capability chargeafter.com/charges.settle { chargeId: string, amount: number, authorizedAmount: number, merchantOrderId: string }
 */
export function settleCharge(chargeId: string, amountValue?: number, merchantTransactionId?: string): ChargeTransaction {
    const id = requiredText(chargeId, "chargeId");
    const requested = amountValue === undefined ? undefined : amount(amountValue, "amount");
    const transaction = optionalText(merchantTransactionId, "merchantTransactionId");
    const charge = requireCharge(id);
    const value = requested ?? charge.authorizedAmount;
    if (value <= 0 || value > charge.authorizedAmount) throw invalidArgument("amount exceeds the authorized amount");
    check("chargeafter.com/charges.settle", { chargeId: id, amount: value, authorizedAmount: charge.authorizedAmount, merchantOrderId: charge.merchantOrderId });
    return transactionFrom(caPost("/v2/post-sale/charges/" + encodeURIComponent(id) + "/settles", transactionBody(value, transaction)).json() as ChargeTransactionWire);
}

/** Void a charge.
 * @param chargeId Charge identifier.
 * @param amountValue Optional void amount.
 * @param merchantTransactionId Optional merchant transaction reference.
 * @returns The created void transaction.
 * @capability chargeafter.com/charges.void { chargeId: string, amount: number, state: string }
 */
export function voidCharge(chargeId: string, amountValue?: number, merchantTransactionId?: string): ChargeTransaction {
    const id = requiredText(chargeId, "chargeId");
    const requested = amountValue === undefined ? undefined : amount(amountValue, "amount");
    const transaction = optionalText(merchantTransactionId, "merchantTransactionId");
    const charge = requireCharge(id);
    const value = requested ?? charge.authorizedAmount;
    if (value <= 0 || value > charge.authorizedAmount) throw invalidArgument("amount exceeds the authorized amount");
    check("chargeafter.com/charges.void", { chargeId: id, amount: value, state: charge.state });
    return transactionFrom(caPost("/v2/post-sale/charges/" + encodeURIComponent(id) + "/voids", transactionBody(value, transaction)).json() as ChargeTransactionWire);
}

/** Reads authoritative settled and refunded amounts before the policy check and refund request. */
/** Refund a settled charge.
 * @param chargeId Charge identifier.
 * @param amountValue Refund amount.
 * @param reason Optional reason. ChargeAfter does not accept it, so a supplied value is rejected before any request.
 * @param merchantTransactionId Optional merchant transaction reference.
 * @returns The created refund transaction.
 * @capability chargeafter.com/charges.refund { chargeId: string, amount: number, settledAmount: number }
 */
export function refundCharge(chargeId: string, amountValue: number, reason?: string, merchantTransactionId?: string): ChargeTransaction {
    const id = requiredText(chargeId, "chargeId");
    const value = amount(amountValue, "amount");
    const refundReason = optionalText(reason, "reason");
    if (refundReason !== undefined) throw invalidArgument("ChargeAfter refund API does not accept a reason; record it in escalation and omit it here");
    const transaction = optionalText(merchantTransactionId, "merchantTransactionId");
    const charge = requireCharge(id);
    if (value > charge.settledAmount - charge.refundedAmount) throw invalidArgument("amount exceeds the remaining refundable amount");
    check("chargeafter.com/charges.refund", { chargeId: id, amount: value, settledAmount: charge.settledAmount });
    return transactionFrom(caPost("/v2/post-sale/charges/" + encodeURIComponent(id) + "/refunds", transactionBody(value, transaction)).json() as ChargeTransactionWire);
}

/** Set the merchant order ID of a charge.
 * @param chargeId Charge identifier.
 * @param merchantOrderId New merchant order ID.
 * @returns The updated charge.
 * @capability chargeafter.com/charges.update { chargeId: string }
 */
export function setChargeOrderId(chargeId: string, merchantOrderId: string): Charge {
    const id = requiredText(chargeId, "chargeId");
    const orderId = requiredText(merchantOrderId, "merchantOrderId");
    if (orderId.length > 256) throw invalidArgument("merchantOrderId must be at most 256 characters");
    check("chargeafter.com/charges.update", { chargeId: id });
    return chargeFrom(caPatch("/v2/post-sale/charges/" + encodeURIComponent(id), { merchantOrderId: orderId }).json() as ChargeWire);
}

/** List one bounded funding-report page.
 * @param input Date range and page options.
 * @returns Funding records.
 * @capability chargeafter.com/fundings.list { fromDate: string, toDate: string }
 */
export function listFundings(input: FundingQuery): FundingPage {
    const fromDate = date(input.fromDate, "fromDate");
    const toDate = date(input.toDate, "toDate");
    const window = page(input.offset, input.limit);
    check("chargeafter.com/fundings.list", { fromDate: fromDate, toDate: toDate });
    const query = "?fromDate=" + encodeURIComponent(fromDate) + "&toDate=" + encodeURIComponent(toDate) + "&offset=" + window.offset.toString() + "&limit=" + window.limit.toString() + "&showMissing=" + (input.showMissing === true ? "true" : "false");
    const data = caGet("/v2/post-sale/fundings/report" + query).json() as { records?: FundingWire[] };
    const records: FundingRecord[] = [];
    for (const record of data.records ?? []) records.push(fundingFrom(record));
    return { records: records, offset: window.offset, limit: window.limit };
}

/** Look up consumer accounts by contact details.
 * @param input Email and/or phone.
 * @returns The lookup result, or null when absent.
 * @capability chargeafter.com/consumers.lookup { email: string, phone: string }
 */
export function lookupConsumer(input: ConsumerLookup): ConsumerLookupResult | null {
    const firstName = requiredText(input.firstName, "firstName"); const lastName = requiredText(input.lastName, "lastName"); const email = requiredText(input.email, "email"); const phone = optionalText(input.phone, "phone");
    const shippingAddress = address(input.shippingAddress, "shippingAddress"); const billingAddress = address(input.billingAddress, "billingAddress"); const netMonthlyIncome = finite(input.netMonthlyIncome, "netMonthlyIncome"); const dateOfBirth = requiredText(input.dateOfBirth, "dateOfBirth");
    check("chargeafter.com/consumers.lookup", { email: email, phone: phone ?? "" });
    const consumerDetails: Record<string, unknown> = { firstName: firstName, lastName: lastName, email: email, shippingAddress: shippingAddress, billingAddress: billingAddress, netMonthlyIncome: netMonthlyIncome, dateOfBirth: dateOfBirth }; if (phone !== undefined) consumerDetails.mobilePhoneNumber = phone;
    const response = nullable(caPostRaw("/v2/consumers/consumer-details", { consumerDetails: consumerDetails }));
    if (response === null) return null;
    const result = response.json() as ConsumerLookupWire;
    return { applicationId: text(result.applicationId, "applicationId"), accounts: consumerAccountsFrom(result.accountsInfo ?? []) };
}

/** List known accounts for a consumer.
 * @param consumerId Consumer identifier.
 * @returns Consumer accounts.
 * @capability chargeafter.com/consumers.accounts { consumerId: string }
 */
export function getConsumerAccounts(consumerId: string): ConsumerAccount[] {
    const id = requiredText(consumerId, "consumerId");
    check("chargeafter.com/consumers.accounts", { consumerId: id });
    const data = consumerAccountsResponse(caGet("/v2/consumers/" + encodeURIComponent(id) + "/accounts").json());
    return consumerAccountsFrom(data.accountsInfo ?? []);
}

function fetchCharge(id: string): Charge | null { const response = nullable(caGetRaw("/v2/post-sale/charges/" + encodeURIComponent(id))); return response === null ? null : chargeFrom(response.json() as ChargeWire); }
function requireCharge(id: string): Charge { const charge = fetchCharge(id); if (charge === null) throw new ChargeAfterError("2001", "ChargeAfter charge was not found", 400, false); return charge; }
function caGet(path: string): Response { return requireOk(caGetRaw(path)); }
function caGetRaw(path: string): Response { return get(baseUrl + "/" + path.slice(1), headers()); }
function caPost(path: string, body: Record<string, unknown>): Response { return requireOk(caPostRaw(path, body)); }
function caPostRaw(path: string, body: Record<string, unknown>): Response { return post(baseUrl + "/" + path.slice(1), body, headers()); }
function caPatch(path: string, body: Record<string, unknown>): Response { return requireOk(patch(baseUrl + "/" + path.slice(1), body, headers())); }
function headers(): Map<string, string> { const key = secrets.get("CHARGEAFTER_PRIVATE_KEY"); if (key === undefined || key.length === 0) throw new ChargeAfterError("missing_private_key", "CHARGEAFTER_PRIVATE_KEY is not bound", 0, false); const headers = new Map<string, string>(); headers.set("Authorization", "Bearer " + key); headers.set("Accept", "application/json"); headers.set("Content-Type", "application/json"); return headers; }
function transactionBody(value: number, transaction: string | undefined): Record<string, unknown> { const body: Record<string, unknown> = { amount: value }; if (transaction !== undefined) { if (transaction.length > 256) throw invalidArgument("merchantTransactionId must be at most 256 characters"); body.merchantTransactionId = transaction; } return body; }
function prepareLink(first: string, last: string, rawEmail: string | undefined, rawPhone: string | undefined, rawMethod: DeliveryMethod, rawFormat: string | undefined, rawStoreId: string, rawChannel: ApplicationChannel): Record<string, unknown> {
    const firstName = requiredText(first, "consumer.firstName");
    const lastName = requiredText(last, "consumer.lastName");
    const email = optionalText(rawEmail, "consumer.email");
    const phone = optionalText(rawPhone, "consumer.phone");
    if (email === undefined && phone === undefined) throw invalidArgument("consumer.email or consumer.phone is required");
    const storeId = requiredText(rawStoreId, "storeId");
    if (rawMethod !== "Email" && rawMethod !== "Sms" && rawMethod !== "None") throw invalidArgument("deliveryMethod must be Email, Sms, or None");
    if (rawChannel !== "E_COMMERCE" && rawChannel !== "IN_STORE" && rawChannel !== "IN_HOME" && rawChannel !== "CALL_CENTER") throw invalidArgument("channel is invalid");
    const consumer: Record<string, unknown> = { firstName: firstName, lastName: lastName };
    if (email !== undefined) consumer.email = email;
    if (phone !== undefined) consumer.mobilePhoneNumber = phone;
    const linkOptions: Record<string, unknown> = { deliveryMethod: rawMethod };
    const format = optionalText(rawFormat, "linkFormat");
    if (format !== undefined) linkOptions.linkFormat = format;
    return { applicationDetails: { channel: rawChannel }, consumer: consumer, merchantDetails: { storeId: storeId }, linkOptions: linkOptions };
}
function prepareCart(rawItems: CartItem[], rawTotal: number, rawTax: number | undefined, rawShipping: number | undefined): Record<string, unknown> {
    const totalAmount = amount(rawTotal, "cart.totalAmount", true);
    if (rawItems.length === 0) throw invalidArgument("cart.items must not be empty");
    const items: Record<string, unknown>[] = [];
    for (const item of rawItems) {
        if (!Number.isInteger(item.quantity) || item.quantity < 1) throw invalidArgument("item.quantity must be a positive integer");
        items.push({ sku: requiredText(item.sku, "item.sku"), name: requiredText(item.name, "item.name"), quantity: item.quantity, price: amount(item.unitPrice, "item.unitPrice", true) });
    }
    const result: Record<string, unknown> = { items: items, totalAmount: totalAmount };
    if (rawTax !== undefined) result.totalTaxAmount = amount(rawTax, "cart.taxAmount", true);
    if (rawShipping !== undefined) result.shippingAmount = amount(rawShipping, "cart.shippingAmount", true);
    return result;
}

interface GeneratedLinkWire { linkId?: string; url?: string | null; qrCode?: string | null; }
interface LinkDataWire { linkStatus?: string; linkType?: string; createdAt?: string | null; updatedAt?: string | null; merchantDetails?: { storeId?: string } | null; }
interface ApplicationStatusWire { sessionStatus?: string; flowType?: string; applicationStatus?: string | null; storeId?: string | null; account?: { lenderId?: string; lenderName?: string; creditInfo?: { availableCredit?: number } | null } | null; }
interface ApplicationOfferWire { id?: string; lenderId?: string; lenderName?: string; status?: string; creditInfo?: { availableCredit?: number } | null; offers?: OfferWire[] | null; }
interface OfferWire { amount?: number; code?: string; }
interface ChargeWire { id?: string; state?: string; totalAmount?: number; settledAmount?: number; refundedAmount?: number; merchantOrderId?: string; storeId?: string | null; }
interface ChargeTransactionWire { id?: string; chargeId?: string; amount?: number; merchantOrderId?: string; status?: string; createdAt?: string; updatedAt?: string; lenderId?: string; merchantId?: string; type?: string; }
interface FundingWire { chargeId?: string; transactionId?: string; transactionType?: string; transactionAmount?: number; merchantOrderId?: string; merchantTransactionId?: string | null; status?: string; fundedAt?: string | null; lenderName?: string | null; netAmount?: number | null; }
interface ConsumerLookupWire { applicationId?: string; accountsInfo?: ConsumerAccountInfoWire[] | null; }
function text(value: string | undefined, field: string): string { if (value === undefined || value.length === 0) throw new ChargeAfterError("invalid_response", "ChargeAfter response is missing " + field, 0, false); return value; }
function number(value: number | undefined, field: string): number { if (value === undefined || !Number.isFinite(value)) throw new ChargeAfterError("invalid_response", "ChargeAfter response is missing " + field, 0, false); return value; }
function generatedLinkFrom(data: GeneratedLinkWire): GeneratedLink { return { linkId: text(data.linkId, "linkId"), url: data.url ?? null, qrCode: data.qrCode ?? null }; }
function linkDataFrom(data: LinkDataWire): LinkData { return { status: data.linkStatus ?? "", linkType: data.linkType ?? "", createdAt: data.createdAt ?? null, updatedAt: data.updatedAt ?? null, storeId: data.merchantDetails?.storeId ?? null }; }
function applicationStatusFrom(applicationId: string, data: ApplicationStatusWire): ApplicationStatus { const account = data.account; return { applicationId: applicationId, sessionStatus: text(data.sessionStatus, "sessionStatus"), flowType: text(data.flowType, "flowType"), applicationStatus: data.applicationStatus ?? null, lenderId: account?.lenderId ?? null, lenderName: account?.lenderName ?? null, availableCredit: account?.creditInfo?.availableCredit ?? null, storeId: data.storeId ?? null }; }
function applicationOfferFrom(account: ApplicationOfferWire, offer: OfferWire): ApplicationOffer { return { accountId: text(account.id, "account id"), lenderId: account.lenderId ?? "", lenderName: account.lenderName ?? "", status: account.status ?? "", amount: number(offer.amount, "offer amount"), availableCredit: account.creditInfo?.availableCredit ?? null, promoCode: offer.code ?? null }; }
function chargeFrom(data: ChargeWire): Charge { return { id: text(data.id, "charge id"), state: text(data.state, "state"), authorizedAmount: number(data.totalAmount, "totalAmount"), settledAmount: number(data.settledAmount, "settledAmount"), refundedAmount: number(data.refundedAmount, "refundedAmount"), merchantOrderId: text(data.merchantOrderId, "merchantOrderId"), storeId: data.storeId ?? null }; }
function transactionFrom(data: ChargeTransactionWire): ChargeTransaction { return { id: text(data.id, "transaction id"), chargeId: text(data.chargeId, "chargeId"), amount: number(data.amount, "amount"), merchantOrderId: text(data.merchantOrderId, "merchantOrderId"), status: text(data.status, "status"), createdAt: text(data.createdAt, "createdAt"), updatedAt: text(data.updatedAt, "updatedAt"), lenderId: text(data.lenderId, "lenderId"), merchantId: text(data.merchantId, "merchantId"), type: data.type ?? null }; }
function fundingFrom(data: FundingWire): FundingRecord { return { chargeId: data.chargeId ?? null, transactionId: data.transactionId ?? null, transactionType: data.transactionType ?? "", transactionAmount: number(data.transactionAmount, "transactionAmount"), merchantOrderId: data.merchantOrderId ?? "", merchantTransactionId: data.merchantTransactionId ?? null, status: data.status ?? "", fundedAt: data.fundedAt ?? null, lenderName: data.lenderName ?? null, netAmount: data.netAmount ?? null }; }
function address(value: Address, field: string): Record<string, unknown> { const result: Record<string, unknown> = { state: requiredText(value.state, field + ".state"), city: requiredText(value.city, field + ".city"), line1: requiredText(value.line1, field + ".line1"), zipCode: requiredText(value.zipCode, field + ".zipCode") }; const line2 = optionalText(value.line2, field + ".line2"); if (line2 !== undefined) result.line2 = line2; return result; }
function finite(value: number, field: string): number { if (!Number.isFinite(value)) throw invalidArgument(field + " must be finite"); return value; }
function consumerAccountsFrom(data: ConsumerAccountInfoWire[]): ConsumerAccount[] { const result: ConsumerAccount[] = []; for (const item of data) { const account = item.account; if (account === undefined) throw new ChargeAfterError("invalid_response", "ChargeAfter response is missing account", 0, false); result.push({ id: text(account.id, "account id"), lenderId: account.lenderId ?? "", lenderName: account.lenderName ?? "", status: account.status ?? "", availableCredit: account.creditInfo?.availableCredit ?? null }); } return result; }
