// SUB-1822: ReadonlyMap and ReadonlySet, as in tsc's lib: every Map or Set is
// one, and they offer the reading members only.
function total(m: ReadonlyMap<string, number>): number {
  let sum = 0;
  for (const [, v] of m) {
    sum += v;
  }
  m.forEach((v) => {
    sum += v;
  });
  return sum + m.size + (m.get("a") ?? 0) + (m.has("b") ? 1 : 0);
}
function members(s: ReadonlySet<number>): number[] {
  return Array.from(s.values()).concat(s.has(1) ? [1] : []);
}
function main(): void {
  const m = new Map<string, number>([["a", 1], ["b", 2]]);
  assert(total(m) === 10);
  const s = new Set([1, 2]);
  assert(members(s).join(",") === "1,2,1");
  const view: ReadonlySet<number> = s;
  s.add(3);
  assert(view.size === 3 && view.union(new Set([4])).size === 4);
  const empty: ReadonlyMap<string, boolean> = new Map();
  assert(empty.size === 0);
  const none: readonly number[] = Array.of();
  assert(none.length === 0);
}
