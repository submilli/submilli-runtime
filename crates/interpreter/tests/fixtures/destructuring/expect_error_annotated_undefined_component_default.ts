// expect-error: expected `undefined`, got `number`
// expect-error-count: 1
// An annotated component that can only be `undefined` still checks its
// default, as tsc does (TS2322); only an unannotated one types it alone.
function g({ a = 1 }: { a: undefined }): void {
  console.log(a);
}
function main(): void {
  g({ a: undefined });
}
