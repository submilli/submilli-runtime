// AttioError and the local-validation shorthand. `src/lib.ts` re-exports
// AttioError; the rest of this module is internal.

/**
 * An Attio API error (the HTTP status of Attio's error response), or an error
 * the package raised itself (status 0): invalid input, refused before any
 * request; an unknown status title; a status attribute passed to a plain write;
 * or an ambiguous match.
 */
export class AttioError extends Error {
    /** Attio's error `code` (such as `value_not_found`), a code mapped from the HTTP status, or the package's own code. */
    code: string;
    /** HTTP status of Attio's error response, or 0 for an error the package raised itself. */
    status: number;
    /** Attio's error `type` (such as `invalid_request_error`), or "" when absent. */
    type: string;
    /** True when Attio did not apply the request and the same call may be retried: 429 rate limits and 409 `concurrent_write_conflict`. */
    retryable: boolean;
    /**
     * Seconds to wait, from Attio's `Retry-After` header: its number of seconds,
     * or the time from when the error was raised until its HTTP date (0 when that
     * has passed). Null when there is no valid header, as for a 409
     * `concurrent_write_conflict`, which is retryable without one. A program
     * cannot sleep, so it should stop and report rather than retry at once.
     */
    retryAfterSeconds: number | null;

    constructor(
        code: string,
        message: string,
        status: number,
        type: string,
        retryable: boolean,
        retryAfterSeconds: number | null,
    ) {
        super(message);
        this.name = "AttioError";
        this.code = code;
        this.status = status;
        this.type = type;
        this.retryable = retryable;
        this.retryAfterSeconds = retryAfterSeconds;
    }
}

export function validationError(code: string, message: string): AttioError {
    return new AttioError(code, message, 0, "", false, null);
}
