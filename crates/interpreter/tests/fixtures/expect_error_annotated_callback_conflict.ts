// A fully annotated callback binds `U` from its own annotations, in argument
// order, so the mismatch is reported once, at the initial value, as in tsc.
// expect-error: expected `string`, got `number`
// expect-error-count: 1
function fold<T, U>(xs: T[], f: (acc: U, x: T) => U, init: U): U {
  let a = init;
  for (const x of xs) {
    a = f(a, x);
  }
  return a;
}
function main(): void {
  console.log(fold([1, 2], (acc: string, x: number): string => acc, 0));
}
