import { ChargeAfterError } from "./errors";
import { Response } from "submilli:http";

/** Internal wire model for the documented nested consumer account response. */
export interface ConsumerAccountWire { id?: string; lenderId?: string; lenderName?: string; status?: string; creditInfo?: { availableCredit?: number } | null; }
/** Internal account-info envelope. */
export interface ConsumerAccountInfoWire { account?: ConsumerAccountWire; }
/** Internal consumer account response, including documented nullable accountsInfo. */
export interface ConsumerAccountsResponseWire { accountsInfo?: ConsumerAccountInfoWire[] | null; }

/** Runtime-checked internal response cast shared by both consumer endpoints.
 * @param value Parsed response value.
 * @returns Typed internal consumer account response.
 */
export function consumerAccountsResponse(value: unknown): ConsumerAccountsResponseWire { return value as ConsumerAccountsResponseWire; }

export function requireOk(response: Response): Response {
    if (response.ok) return response;
    throw errorFrom(response);
}

export function nullable(response: Response): Response | null {
    if (response.ok) return response;
    const error = errorFrom(response);
    if (error.code === "2001" || error.status === 404) return null;
    throw error;
}

export function errorFrom(response: Response): ChargeAfterError {
    let code = codeForStatus(response.status);
    let message = "ChargeAfter request failed with HTTP " + response.status.toString();
    // Responses have varied between { code, message } and validation envelopes.
    // Surface only an API code and short message: never return a request body or headers.
    try {
        const body = response.json() as { code?: string | number, message?: string, error?: { code?: string | number, message?: string }, errors?: { code?: string | number, description?: string, message?: string }[] };
        const nested = body.error;
        const firstError = body.errors === undefined ? undefined : body.errors[0];
        const rawCode = nested?.code ?? firstError?.code ?? body.code;
        const rawMessage = nested?.message ?? firstError?.description ?? firstError?.message ?? body.message;
        if (typeof rawCode === "number") code = rawCode.toString();
        else if (typeof rawCode === "string") code = rawCode;
        if (rawMessage !== undefined && rawMessage.length > 0) message = rawMessage;
    } catch (error) {}
    return new ChargeAfterError(code, message, response.status, response.status === 429 || response.status >= 500);
}

function codeForStatus(status: number): string {
    if (status === 401) return "unauthorized";
    if (status === 403) return "forbidden";
    if (status === 404) return "not_found";
    if (status === 422) return "validation_failed";
    if (status === 429) return "rate_limited";
    if (status >= 500) return "server_error";
    return "http_error";
}
