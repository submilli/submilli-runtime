// expect-error: expected `number`, got `number | string`
function pick(): boolean { return true; }
function main(): void {
  const a = { a: "text" };
  const merged = { a: 123, ...(pick() ? a : {}) };
  const n: number = merged.a;
}
