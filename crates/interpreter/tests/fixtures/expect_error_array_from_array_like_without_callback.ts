// expect-error: has no elements, so it makes an array of `undefined`, not of `number`
// expect-error-count: 2
// The elements would arrive as `null` in a `number` slot, where JavaScript
// has `undefined`; the program is refused instead.
function main(): void {
  const fromContext: number[] = Array.from({ length: 2 });
  const fromTypeArgument = Array.from<number>({ length: 2 });
  console.log(fromContext.length + fromTypeArgument.length);
}
