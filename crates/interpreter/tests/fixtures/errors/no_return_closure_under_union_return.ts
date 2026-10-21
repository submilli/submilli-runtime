// expect-error: expected `() => string | undefined`, got `() => void`
// A block body with no `return` returns `undefined` only where the context's
// return type is `undefined` itself, as in TypeScript 5.1; under any union it
// returns `void`, which a `string | undefined` result doesn't take.
const done: () => undefined = () => {};
const either: (() => void) | (() => string) = () => {};
const maybe: () => string | undefined = () => {};

function main(): void {}
