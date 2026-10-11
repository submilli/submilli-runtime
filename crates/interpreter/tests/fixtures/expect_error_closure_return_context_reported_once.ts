// A return the context rejects is reported once, in the closure, and a cast
// relates a function literal's annotated parameters as a whole, without
// reporting each one.
// expect-error: expected `number`, got `string`
// expect-error: cannot cast `(arg0: string) => number` to `(arg0: number) => number`
// expect-error-count: 2
function main(): void {
  const n: number = (() => "s")();
  const f = ((x: string) => 1) as (x: number) => number;
}
