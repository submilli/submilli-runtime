// Only `unknown` takes a `void` function's result: a function returning
// `void` still doesn't fit where a function returning a value is expected.
// expect-error: expected `() => number | null`, got `() => void`
// expect-error-count: 1
function main(): void {
  const f: () => number | null = () => {};
}
