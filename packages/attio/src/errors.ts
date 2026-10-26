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
    /** The raw `Retry-After` header of a 429, as Attio sent it (an HTTP date such as `Tue, 23 May 2023 14:42:01 GMT`), or "" when absent. */
    retryAfter: string;
    /**
     * Seconds from when the error was raised until Attio's `Retry-After` time
     * (0 when that time has passed), or null when the header is absent or not a
     * date or a number of seconds. A program cannot sleep, so it should stop and
     * report rather than retry at once.
     */
    retryAfterSeconds: number | null;

    constructor(
        code: string,
        message: string,
        status: number,
        type: string,
        retryable: boolean,
        retryAfter: string,
        retryAfterSeconds: number | null,
    ) {
        super(message);
        this.name = "AttioError";
        this.code = code;
        this.status = status;
        this.type = type;
        this.retryable = retryable;
        this.retryAfter = retryAfter;
        this.retryAfterSeconds = retryAfterSeconds;
    }
}

export function validationError(code: string, message: string): AttioError {
    return new AttioError(code, message, 0, "", false, "", null);
}
