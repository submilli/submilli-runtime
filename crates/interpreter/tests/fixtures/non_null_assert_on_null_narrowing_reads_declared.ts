// `x!` where a write narrowed `x` to `null` reads `x` at its declared type, as
// in TypeScript, so a member read after it type-checks; the `!` throws first.
let g: string | null = "g";
type Box = { v: string | null };
function local(): string {
  let x: string | null = "a";
  x = null;
  return x!.slice(0);
}
function global(): string {
  g = null;
  return g!.slice(0);
}
function field(b: Box): string {
  b.v = null;
  return b.v!.slice(0);
}
function chained(b: Box): string {
  if (b.v === null) {
    const sliced = b?.v!.slice(0);
    return sliced ?? "absent";
  }
  return "skipped";
}
function tryIt(f: () => string): string {
  try { return f(); } catch (e) { return e instanceof TypeError ? "TypeError" : "other"; }
}
function main(): void {
  assert(tryIt(local) === "TypeError");
  assert(tryIt(global) === "TypeError");
  assert(tryIt(() => field({ v: "x" })) === "TypeError");
  assert(tryIt(() => chained({ v: null })) === "TypeError");
}
