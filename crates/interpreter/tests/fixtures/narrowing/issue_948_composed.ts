function isNullableString(x: unknown): x is string | null {
    if (x === null) { return true; }
    return typeof x === "string";
}
function nonNull<T>(x: T): T {
    if (isNullableString(x)) { return x!; }
    return x;
}
function truthy<T>(x: T): T {
    if (isNullableString(x)) { return x || x; }
    return x;
}
export function main(): void {
    assert(nonNull("hi") === "hi");
    assert(truthy("hi") === "hi");
    assert(truthy(null) === null);
}
