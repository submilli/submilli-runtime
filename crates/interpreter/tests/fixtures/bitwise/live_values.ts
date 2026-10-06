let current: number | null = 7;
function clear(): boolean { current = null; return false; }
function calculate(): number {
  if (current === null || clear()) return 99;
  return current | 3;
}
let big: bigint | number = 7n;
function replace(): boolean { big = 3; return false; }
function complementLive(): bigint {
  if (typeof big === "number" || replace()) return 0n;
  return ~big;
}
function main(): void {
  assert(calculate() === 3, "stale number view coerces live null");
  const result: unknown = complementLive();
  assert(result === -4, "stale bigint view complements live number");
}
