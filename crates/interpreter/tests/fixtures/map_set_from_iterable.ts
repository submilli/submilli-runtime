// Map/Set iterable-initializer constructors: entries/value arrays, another
// collection as the source, duplicate handling, insertion order.
function main(): void {
  const entries: [string, number][] = [["x", 1], ["y", 2], ["x", 3]];
  const m = new Map<string, number>(entries);
  assert(m.size === 2, "duplicate keys collapse");
  assert(m.get("x") === 3, "last write wins");
  assert(m.get("y") === 2, "second key");

  const copy = new Map<string, number>(m);
  assert(copy.size === 2 && copy.get("x") === 3, "map-from-map copies entries");

  const s = new Set<number>([5, 6, 5]);
  assert(s.size === 2 && s.has(5) && s.has(6), "set from array dedupes");

  const s2 = new Set<number>(s);
  assert(s2.size === 2, "set from set copies");

  let order = "";
  for (const v of s) {
    order = order + v.toString() + ";";
  }
  assert(order === "5;6;", "insertion order survives the initializer");
}
