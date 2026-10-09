// A parameter with a default is already optional, so `?` and `= value` together
// are rejected the same way in a function and in an arrow. Parsing continues.
// expect-error: an optional parameter cannot have a default value
// expect-error: an optional parameter cannot have a default value
// expect-error: drop the `?`: a parameter with a default is already optional
// expect-error: rest parameter cannot have a default value
// expect-error-count: 3
function declared(a?: number = 1): void {}

const arrow = (b?: number = 2): void => {};

const rest = (...c: number[] = []): void => {};

function main(): void {}
