/** A safe mapped ChargeAfter or local-validation error. */
export class ChargeAfterError extends Error {
    code: string;
    status: number;
    retryable: boolean;

    constructor(code: string, message: string, status: number, retryable: boolean) {
        super(message);
        this.name = "ChargeAfterError";
        this.code = code;
        this.status = status;
        this.retryable = retryable;
    }
}

export function invalidArgument(message: string): ChargeAfterError {
    return new ChargeAfterError("invalid_argument", message, 0, false);
}
