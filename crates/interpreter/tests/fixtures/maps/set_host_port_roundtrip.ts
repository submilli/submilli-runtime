// End-to-end backstop for the Rust-host Set: object elements compared
// structurally through the vtable, every iterator + forEach, tombstone/ledger
// churn, the host-driven constructor (array / Set / string), and the ES2025
// algebra + relation ops.

function main(): void {
  // Object elements: structural equality on add/has/delete.
  const s = new Set<{ id: number }>();
  const a = { id: 1 };
  const b = { id: 2 };
  s.add(a);
  s.add(b);
  s.add({ id: 1 }); // structural duplicate — a no-op
  assert(s.size === 2, "structural dedup");
  assert(s.has({ id: 1 }), "structural lookup");
  assert(s.delete({ id: 2 }), "delete by structural value");
  assert(!s.has(b), "deleted value is gone");
  assert(s.size === 1, "size reflects the delete");

  // Primitive elements + insertion order through every iterator.
  const n = new Set<number>();
  n.add(1);
  n.add(2);
  n.add(3);
  n.add(2); // duplicate
  assert(n.size === 3, "number dedup");

  let viaForEach: number = 0;
  n.forEach((x: number) => {
    viaForEach = viaForEach + x;
  });
  assert(viaForEach === 6, "forEach visits each element once");

  let viaValues: number = 0;
  for (const x of n.values()) {
    viaValues = viaValues + x;
  }
  assert(viaValues === 6, "values() iterates every element");

  let pairOk: boolean = true;
  for (const e of n.entries()) {
    if (e[0] !== e[1]) {
      pairOk = false;
    }
  }
  assert(pairOk, "entries() yields [v, v] pairs");

  // keys() is an alias of values(); drive its host-built cursor by hand.
  let keyCount: number = 0;
  const it = n.keys();
  while (true) {
    const r = it.next();
    if (r.done) {
      break;
    }
    keyCount = keyCount + 1;
  }
  assert(keyCount === 3, "manual next() over keys()");

  // Delete-then-reinsert churn exercises tombstones + ledger compaction.
  let i: number = 0;
  while (i < 40) {
    n.add(100 + i);
    n.delete(100 + i);
    i = i + 1;
  }
  assert(n.size === 3, "churn leaves the original elements");
  assert(n.has(1) && n.has(2) && n.has(3), "originals survive churn");

  // Constructors: array (dedups), another Set (independent copy), string (code points).
  const fromArray = new Set([1, 2, 2, 3]);
  assert(fromArray.size === 3, "array initializer dedups");
  const copy = new Set(fromArray);
  copy.add(4);
  assert(fromArray.size === 3 && copy.size === 4, "copy is independent");
  const fromString = new Set("abca");
  assert(fromString.size === 3, "string initializer yields code points");

  copy.clear();
  assert(copy.size === 0, "clear empties the set");

  // Algebra + relations.
  const x = new Set([1, 2, 3]);
  const y = new Set([2, 3, 4]);
  assert(x.union(y).size === 4, "union merges both");
  const inter = x.intersection(y);
  assert(inter.size === 2 && inter.has(2) && inter.has(3), "intersection keeps shared");
  assert(x.difference(y).size === 1, "difference drops shared");
  assert(new Set([2, 3]).isSubsetOf(x), "isSubsetOf");
  assert(x.isSupersetOf(new Set([1, 2])), "isSupersetOf");
  assert(x.isDisjointFrom(new Set([7, 8])), "isDisjointFrom");
}
