type Dynamic = unknown;
type PrimitiveText = string;
function read(x: Dynamic): string {
  if (x === "a") { const exact: "a" = x; return exact; }
  if ("b" === x) { return x; }
  return "other";
}
function numberValue(x: unknown): number {
  if (x !== 42) { return 0; } else { const exact: 42 = x; return exact; }
}
function isA(x: unknown): x is "a" { return x === "a"; }
function broad(x: PrimitiveText): string {
  if (x !== "a") { return "other"; }
  const exact: "a" = x;
  return exact;
}
function generic<T>(x: T): string {
  if (typeof x === "string" && x === "a") { return x; }
  return "other";
}
function field(x: { value: unknown }): string {
  if (x.value === "a" || x.value === "b") { return x.value; }
  return "other";
}
function signed(x: unknown): number {
  if (x === -1 || x === +2) { return x; }
  return 0;
}
function arithmetic(x: number): number {
  if (x === 1 || x === 2) { return x + 1; }
  return 0;
}
function concatenate(x: string): string {
  if (x === "a" || x === "b") { return x + "!"; }
  return "other";
}
let globalNumber: number = 1;
function increment(x: number): number {
  if (x === 1 || x === 2) { const previous = x++; return previous * 10 + x; }
  return 0;
}
function incrementField(x: { value: number }): number {
  if (x.value === 1 || x.value === 2) { const previous = x.value++; return previous * 10 + x.value; }
  return 0;
}
function decrementField(x: { value: number }): number {
  if (x.value === 1 || x.value === 2) { return x.value-- * 10 + x.value; }
  return 0;
}
function decrement(): number {
  if (globalNumber === 1 || globalNumber === 2) { const previous = globalNumber--; return previous * 10 + globalNumber; }
  return 0;
}
function main(): void {
  assert(incrementField({ value: 2 }) === 23 && decrementField({ value: 2 }) === 21, "field postfix invalidates literal guard");
  assert(increment(1) === 12 && increment(2) === 23 && decrement() === 10, "postfix literal union");
  assert(arithmetic(1) === 2 && arithmetic(2) === 3 && arithmetic(5) === 0, "literal union arithmetic");
  assert(concatenate("a") === "a!" && concatenate("b") === "b!", "literal union concatenation");
  assert(signed(-1) === -1 && signed(2) === 2 && signed("-1") === 0, "signed numeric literals");
  assert(field({ value: "a" }) === "a" && field({ value: "b" }) === "b" && field({ value: 5 }) === "other", "field literal narrowing");
  assert(read("a") === "a", "strict equality");
  assert(read("b") === "b", "reversed equality");
  assert(read(3) === "other", "nonmatching primitive");
  assert(read(null) === "other", "null");
  assert(numberValue(42) === 42 && numberValue("42") === 0, "numeric literal and inequality");
  assert(isA("a") && !isA("b"), "predicate validation");
  assert(broad("a") === "a" && broad("b") === "other", "broad alias");
  assert(generic("a") === "a" && generic(5) === "other", "generic refinement");
}
