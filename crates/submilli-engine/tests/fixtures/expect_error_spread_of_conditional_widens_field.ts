// `c ? a : {}` spreads as either object, not their join `{}`: the field holds
// the spread's `string` or the earlier `number`, as TypeScript types it.
// expect-error: expected `number`, got `number | string`
function pick(): boolean {
  return true;
}

function main(): void {
  const a: { a: string } = { a: "text" };
  const merged = { a: 123, ...(pick() ? a : {}) };
  const n: number = merged.a;
  console.log(n);
}
