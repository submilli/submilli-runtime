// ShopifyError and the local-validation shorthand. `src/lib.ts` re-exports
// ShopifyError; the rest of this module is internal.

/**
 * A Shopify API error, or an error the package raised itself (status 0):
 * invalid input refused before any request, a missing secret, or an answer
 * the package could not read.
 */
export class ShopifyError extends Error {
    /** Shopify's error code when it sent one, a code mapped from the HTTP status, or the package's own code. */
    code: string;
    /** HTTP status of Shopify's error response, 200 for a GraphQL or user error, or 0 for an error the package raised itself. */
    status: number;
    /** True when the request was throttled, so Shopify did not apply it and the same call may be retried later. */
    retryable: boolean;
    /** Seconds to wait before retrying, or null when Shopify gave nothing to work it out from. A program cannot sleep, so it should stop and report. */
    retryAfterSeconds: number | null;
    /** The input field Shopify blames, joined with dots, or null. */
    field: string | null;

    constructor(code: string, message: string, status: number, retryable: boolean, retryAfterSeconds: number | null, field: string | null) {
        super(message);
        this.name = "ShopifyError";
        this.code = code;
        this.status = status;
        this.retryable = retryable;
        this.retryAfterSeconds = retryAfterSeconds;
        this.field = field;
    }
}

export function validationError(code: string, message: string): ShopifyError {
    return new ShopifyError(code, message, 0, false, null, null);
}
