// PostHogError and its constructors. `src/lib.ts` re-exports PostHogError;
// the rest of this module is internal.

/** A PostHog API or local validation error. */
export class PostHogError extends Error {
    /** Stable code: `invalid_input`, `missing_credentials`, `http_<status>`, `budget_exceeded`, or `invalid_response`. */
    code: string;
    /** HTTP status, or 0 for a local validation error. */
    status: number;
    /** Seconds PostHog asked the caller to wait before retrying, when it said so. */
    retryAfterSeconds: number | null;

    constructor(code: string, message: string, status: number, retryAfterSeconds: number | null) {
        super(message);
        this.name = "PostHogError";
        this.code = code;
        this.status = status;
        this.retryAfterSeconds = retryAfterSeconds;
    }
}

/**
 * A local validation error.
 * @param message What was wrong.
 * @returns The error.
 */
export function invalidInput(message: string): PostHogError {
    return new PostHogError("invalid_input", message, 0, null);
}

/**
 * An unexpected-response error.
 * @param message What was wrong.
 * @returns The error.
 */
export function invalidResponse(message: string): PostHogError {
    return new PostHogError("invalid_response", message, 0, null);
}

/**
 * The error for a failed HTTP response.
 * @param status HTTP status.
 * @param body Response body; at most 300 characters are kept.
 * @param retryAfter The Retry-After header, if any.
 * @returns The error, with code `budget_exceeded` for PostHog's exhausted query budget.
 */
export function httpError(status: number, body: string, retryAfter: string | undefined | null): PostHogError {
    const seconds = retryAfter === undefined || retryAfter === null || !/^[0-9]{1,6}$/.test(retryAfter) ? null : parseInt(retryAfter, 10);
    const detail = body.length > 300 ? body.substring(0, 300) : body;
    const code = status === 429 && detail.includes("api_queries_budget_exceeded") ? "budget_exceeded" : "http_" + status.toString();
    return new PostHogError(code, "PostHog returned HTTP " + status.toString() + ": " + detail, status, seconds);
}

/**
 * The error for a secret the blueprint did not supply.
 * @param label Which key is missing, for the message.
 * @returns The error.
 */
export function missingCredentials(label: string): PostHogError {
    return new PostHogError("missing_credentials", "The PostHog " + label + " is not configured for this blueprint", 0, null);
}
