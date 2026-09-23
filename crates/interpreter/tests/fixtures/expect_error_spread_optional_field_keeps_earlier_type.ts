// A spread's optional field may be absent, leaving the earlier value, so the
// field's type includes both: `{ a: 123, ...b }` with `b: { a?: string }` is
// `{ a: number | string }`, as in TypeScript.
// expect-error: expected `string`, got `number | string`
function main(): void {
  const b: { a?: string } = {};
  const merged = { a: 123, ...b };
  const s: string = merged.a;
  console.log(s);
}
