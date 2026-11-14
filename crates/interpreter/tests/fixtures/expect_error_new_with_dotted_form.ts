// A construct signature is no member: `Uint8Array.new(…)` is not `new
// Uint8Array(…)`, as in TypeScript (TS2339).
// expect-error: construct with `new Uint8Array(…)`
function main(): void {
  const b: Uint8Array = Uint8Array.new([1, 2, 3, 4]);
  assert(b.length === 4, "length");
}
