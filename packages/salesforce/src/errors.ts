// SalesforceError and the local-validation shorthand. `src/lib.ts` re-exports
// SalesforceError; the rest of this module is internal.

/**
 * A Salesforce API error (the HTTP status of Salesforce's answer), or an error
 * the package raised itself (status 0): input refused before any request, a
 * missing credential, or an org URL that is not a Salesforce host.
 */
export class SalesforceError extends Error {
    /**
     * Salesforce's `errorCode`, such as `REQUIRED_FIELD_MISSING`, `INVALID_FIELD`,
     * `MALFORMED_QUERY`, `INVALID_SESSION_ID` (an expired or revoked token) or
     * `REQUEST_LIMIT_EXCEEDED`; a code mapped from the HTTP status when the answer
     * has none; or the package's own code, such as `invalid_input`.
     */
    errorCode: string;
    /** The fields Salesforce names in the error, such as `["Amount"]`; empty when none. */
    fields: string[];
    /** HTTP status of Salesforce's answer, or 0 for an error the package raised itself. */
    status: number;
    /**
     * True when Salesforce did not apply the request and the same call may succeed
     * later: `REQUEST_LIMIT_EXCEEDED`, `UNABLE_TO_LOCK_ROW` and HTTP 429. A program
     * cannot sleep, so it should stop and report rather than retry at once.
     */
    retryable: boolean;

    constructor(errorCode: string, message: string, fields: string[], status: number, retryable: boolean) {
        super(message);
        this.name = "SalesforceError";
        this.errorCode = errorCode;
        this.fields = fields;
        this.status = status;
        this.retryable = retryable;
    }
}

export function validationError(code: string, message: string): SalesforceError {
    return new SalesforceError(code, message, [], 0, false);
}
