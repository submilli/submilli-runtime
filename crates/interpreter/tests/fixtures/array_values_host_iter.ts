// Array#values() is built entirely host-side (Rust): the cursor, the `next`
// closure whose funcref is a host Func, and the IteratorResult objects. This
// fixture proves the guest consumes that host-produced iterator — via for-of
// (which exercises value extraction) and via explicit .next()/.done.

function main(): void {
  let sum = 0;
  for (const x of [1, 2, 3].values()) {
    sum = sum + x;
  }
  assert(sum === 6, "host-built Array#values consumed by for-of");

  // Explicit driving: count yields until done (mirrors the Map iterator idiom).
  const it = [10, 20].values();
  const first = it.next();
  assert(!first.done, "explicit next yields before the end");
  let visits = 1;
  while (true) {
    const r = it.next();
    if (r.done) {
      break;
    }
    visits = visits + 1;
  }
  assert(visits === 2, "explicit next walks every element then reports done");

  const empty: number[] = [];
  let count = 0;
  for (const x of empty.values()) {
    count = count + x;
  }
  assert(count === 0, "empty array yields nothing");
}
