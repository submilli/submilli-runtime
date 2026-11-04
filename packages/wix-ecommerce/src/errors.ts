// WixError and the local-validation shorthand. `src/lib.ts` re-exports WixError;
// the rest of this module is internal.

/**
 * A Wix API error (the HTTP status of Wix's error response), or an error the
 * package raised itself (status 0): invalid input refused before any request,
 * a missing secret, an amount above what can be refunded, or an answer the
 * package could not read.
 */
export class WixError extends Error {
    /** Wix's application error code when it sent one, a code mapped from the HTTP status, or the package's own code. */
    code: string;
    /** HTTP status of Wix's error response, or 0 for an error the package raised itself. */
    status: number;
    /** True for 429: Wix did not apply the request and the same call may be retried later. */
    retryable: boolean;
    /**
     * Seconds from Wix's `Retry-After` header, or null without one. Wix does
     * not publish its limits; after a 429 without this header, wait about a
     * minute. A program cannot sleep, so it should stop and report.
     */
    retryAfterSeconds: number | null;

    constructor(code: string, message: string, status: number, retryable: boolean, retryAfterSeconds: number | null) {
        super(message);
        this.name = "WixError";
        this.code = code;
        this.status = status;
        this.retryable = retryable;
        this.retryAfterSeconds = retryAfterSeconds;
    }
}

export function validationError(code: string, message: string): WixError {
    return new WixError(code, message, 0, false, null);
}
