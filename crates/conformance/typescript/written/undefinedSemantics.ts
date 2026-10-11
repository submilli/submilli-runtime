// Written: distinct null/undefined values, optional presence, default parameters,
// optional tuples and methods, and void completion in one comparison case.
// @strict: true

function optionalField(value: { a?: string }): string | undefined {
    const read = value.a;
    if ("a" in value) {
        const present = value.a;
    }
    return read;
}

function requiredField(value: { a: string | undefined }): string | undefined {
    return value.a;
}

optionalField({});
optionalField({ a: undefined });
requiredField({ a: undefined });
requiredField({});
optionalField({ a: null });

function defaulted(value: number | null = 7): number | null {
    return value;
}
defaulted();
defaulted(undefined);
defaulted(null);

function optionalArgument(value?: number): number | undefined {
    return value;
}
optionalArgument();
optionalArgument(undefined);

function tuple(value: [first: number, second?: string]): string | undefined {
    const length = value.length;
    return value[1];
}
tuple([1]);
tuple([1, undefined]);
tuple([1, "two"]);

interface OptionalMethod {
    run?(value: number): string;
}
function callOptional(value: OptionalMethod): string | undefined {
    return value.run?.(1);
}

function distinguish(value: string | null | undefined): string {
    if (value === undefined) return "undefined";
    if (value === null) return "null";
    return value;
}

function destructured(value: { a?: number }): number {
    const { a = 1 } = value;
    return a;
}

function completion(): void {}
const result = completion();
const discarded = void defaulted();
const missing: undefined = undefined;
const explicitNull: null = null;
function main(): void {}
