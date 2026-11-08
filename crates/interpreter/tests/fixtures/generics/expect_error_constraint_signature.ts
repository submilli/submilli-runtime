// expect-error: does not satisfy the constraint
interface Numeric<T extends number> { value: T; }
function unconstrained<T>(value: Numeric<T>): T { return value.value; }
function main(): void {}
