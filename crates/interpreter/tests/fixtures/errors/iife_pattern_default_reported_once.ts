// expect-error: object literal is missing required field `b`
// A parameter typed from its argument checks its default as an annotated one
// does, and a default that doesn't fit is reported once, as in tsc.
function main(): void {
  const r = (({ a, b = 2 } = { a: 1 }) => a + b)({ a: 5, b: 6 });
}
