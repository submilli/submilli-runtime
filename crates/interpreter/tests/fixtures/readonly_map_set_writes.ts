// SUB-1822: as in tsc, a ReadonlyMap or ReadonlySet can't be changed, and it
// doesn't stand where a Map or Set is expected.
// expect-error: cannot call `set` on `ReadonlyMap<string, number>`
// expect-error: cannot call `add` on `ReadonlySet<number>`
// expect-error: cannot call `clear` on `ReadonlySet<number>`
// expect-error: expected `Set<number>`, got `ReadonlySet<number>`
function main(): void {
  const m: ReadonlyMap<string, number> = new Map();
  m.set("a", 1);
  const s: ReadonlySet<number> = new Set();
  s.add(1);
  s.clear();
  const writable: Set<number> = s;
}
