// PostHogError and the local-validation shorthand. `src/lib.ts` re-exports
// PostHogError; the rest of this module is internal.

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
