// ZendeskError and the local-validation shorthand. `src/lib.ts` re-exports
// ZendeskError; the rest of this module is internal.

/**
 * A Zendesk API error (the HTTP status of Zendesk's answer), or an error the
 * package raised itself (status 0): input refused before any request, or a
 * missing credential.
 */
export class ZendeskError extends Error {
    /**
     * Zendesk's error, such as `RecordNotFound`, `RecordInvalid`, `Forbidden`,
     * `invalid_token` (an expired or revoked token) or `UpdateConflict` (the
     * ticket changed since it was read); a code mapped from the HTTP status when
     * the answer has none; or the package's own code, such as `invalid_input`.
     */
    errorCode: string;
    /** Per-field messages for `RecordInvalid`, such as `["requester: is invalid"]`; empty when none. */
    details: string[];
    /** HTTP status of Zendesk's answer, or 0 for an error the package raised itself. */
    status: number;
    /**
     * True when Zendesk did not apply the request and the same call may succeed
     * later: HTTP 429 and 503, and `UpdateConflict` once the ticket is read
     * again. A program cannot sleep, so it should stop and report rather than
     * retry at once.
     */
    retryable: boolean;
    /** Seconds Zendesk asked to wait (`Retry-After`), or null. */
    retryAfter: number | null;

    constructor(errorCode: string, message: string, details: string[], status: number, retryable: boolean, retryAfter: number | null) {
        super(message);
        this.name = "ZendeskError";
        this.errorCode = errorCode;
        this.details = details;
        this.status = status;
        this.retryable = retryable;
        this.retryAfter = retryAfter;
    }
}

export function validationError(code: string, message: string): ZendeskError {
    return new ZendeskError(code, message, [], 0, false, null);
}
