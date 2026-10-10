// expect-error: has no elements, so it makes an array of `undefined`, not of `number`
// expect-error-count: 2
// The elements are `undefined`, which a `number` slot can't hold, so the
// program is refused.
function main(): void {
  const fromContext: number[] = Array.from({ length: 2 });
  const fromTypeArgument = Array.from<number>({ length: 2 });
  console.log(fromContext.length + fromTypeArgument.length);
}
