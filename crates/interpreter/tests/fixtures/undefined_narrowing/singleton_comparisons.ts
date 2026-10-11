function read(value: { a?: number }): number {
  const { a = 1 } = value;
  return a;
}
function fallback(value: number | undefined): number {
  if (value === void 0) { return 2; }
  return value;
}
function explicitNull(): null { return null; }
function nullFallback(value: number | null): number {
  if (value === explicitNull()) { return 3; }
  return value;
}
function main(): void {
  assert(read({}) === 1);
  assert(read({ a: undefined }) === 1);
  assert(read({ a: 5 }) === 5);
  assert(fallback(undefined) === 2);
  assert(fallback(5) === 5);
  assert(nullFallback(null) === 3);
  assert(nullFallback(5) === 5);
  let calls = 0;
  const value: number | undefined = 5;
  if (value !== void (calls = calls + 1)) { assert(value === 5); }
  assert(calls === 1, "comparison operand effects still run");
}
