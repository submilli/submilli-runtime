// A conditional choosing between object literals normalizes them as an array
// literal's elements do, as in tsc, through nested conditionals and one level
// down.
function main(): void {
  const c = [1].length > 0;
  const t = c ? { k: 1 } : { k: 2, m: 3 };
  assert(t.k === 1 && (t.m ?? 0) === 0, "one conditional normalizes");
  const u = c ? (c ? { k: 1 } : { k: 2, m: 3 }) : { k: 4 };
  assert((u.m ?? 0) === 0, "a nested conditional normalizes");
  const v = !c ? (c ? { k: 1 } : { k: 2, m: 3 }) : { k: 4, n: "s" };
  assert(v.k === 4 && v.m == null && v.n === "s", "fields from every branch read");
  const w = c ? { a: { x: 1 } } : { a: { y: 1 } };
  assert(w.a.x === 1 && w.a.y == null, "a nested object normalizes");
  const n = c ? { a: 1 } : null;
  assert(n !== null && n.a === 1, "a literal beside null keeps its type");
}
