// SentryError and the local-validation shorthand. `src/lib.ts` re-exports
// SentryError; the rest of this module is internal.

/** A Sentry API or local validation error with request and rate-limit metadata. */
export class SentryError extends Error {
    code: string;
    status: number;
    requestId: string;
    retryAfterSeconds: number | null;
    rateLimit: number | null;
    rateLimitRemaining: number | null;
    rateLimitReset: number | null;
    concurrentLimit: number | null;
    concurrentRemaining: number | null;

    constructor(
        code: string,
        message: string,
        status: number,
        requestId: string,
        retryAfterSeconds: number | null,
        rateLimit: number | null,
        rateLimitRemaining: number | null,
        rateLimitReset: number | null,
        concurrentLimit: number | null,
        concurrentRemaining: number | null,
    ) {
        super(message);
        this.name = "SentryError";
        this.code = code;
        this.status = status;
        this.requestId = requestId;
        this.retryAfterSeconds = retryAfterSeconds;
        this.rateLimit = rateLimit;
        this.rateLimitRemaining = rateLimitRemaining;
        this.rateLimitReset = rateLimitReset;
        this.concurrentLimit = concurrentLimit;
        this.concurrentRemaining = concurrentRemaining;
    }
}

export function validationError(code: string, message: string): SentryError {
    return new SentryError(code, message, 0, "", null, null, null, null, null, null);
}
