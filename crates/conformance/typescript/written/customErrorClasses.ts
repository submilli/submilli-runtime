// Written for Submilli: the one upstream case that extends `Error` also extends
// `Function`, `Object` and the other built-ins, and checks nothing about the
// error class.

class ValidationError extends Error {
    field: string;
    constructor(field: string, message: string) {
        super(message);
        this.field = field;
    }
}

class LimitError extends ValidationError {
    constructor(public limit: number) {
        super("value", `over ${limit}`);
    }
}

function check(value: number): number {
    if (value < 0) throw new ValidationError("value", "negative");
    if (value > 10) throw new LimitError(10);
    return value;
}

try {
    check(11);
} catch (e) {
    if (e instanceof LimitError) {
        let limit = e.limit;
        let field = e.field;
    } else if (e instanceof ValidationError) {
        let field = e.field;
        let message = e.message;
    }
}

let error = new ValidationError("name", "missing");
let asError: Error = error;
let errorName = error.name;
let described = `${error.field}: ${error.message}`;

function main(): void {}
