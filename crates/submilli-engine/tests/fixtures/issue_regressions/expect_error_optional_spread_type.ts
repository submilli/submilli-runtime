// expect-error: expected `string`, got `number | string`
function main(): void {
  const b: { a?: string } = {};
  const merged = { a: 123, ...b };
  const s: string = merged.a;
}
