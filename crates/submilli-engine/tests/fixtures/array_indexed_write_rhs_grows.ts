// `a[i] = f(…)` where `f` grows `a`. `push` swaps in a fresh backing array, so
// the backing read before the RHS is detached by the time the store runs — the
// write would land in a buffer nobody holds, and the bounds check would test
// the stale length. The receiver expression still runs first — that ordering is
// required; only the read of its backing array moves after the RHS.

function grow(a: number[]): number {
  a.push(99);
  return 7;
}

function growTwice(a: number[]): number {
  a.push(50);
  a.push(60);
  return 8;
}

function main(): void {
  const a: number[] = [1, 2, 3];
  a[0] = grow(a);
  assert(a.length === 4, "push landed");
  assert(a[0] === 7, "the store landed in the live backing array");
  assert(a[3] === 99, "pushed element survived the store");

  const b: number[] = [1];
  b[0] = growTwice(b);
  assert(b.length === 3, "both pushes landed");
  assert(b[0] === 8, "store landed after two reallocations");

  // An index that is only in bounds *because* the RHS grew the array: the
  // bounds check reads the current length, not the one captured earlier.
  const c: number[] = [1];
  c[1] = grow(c);
  assert(c.length === 2, "the push made slot 1 exist");
  assert(c[1] === 7, "the store used the grown array's bounds");

  // `unshift` also reallocates, and moves every element while it is at it.
  const e: number[] = [1, 2];
  e[0] = unshift(e);
  assert(e.length === 3, "unshift landed");
  assert(e[0] === 8 && e[1] === 1 && e[2] === 2, "store used the post-unshift buffer");

  // An alias is the same array, so a RHS that grows it through the alias
  // detaches the buffer just the same.
  const f: number[] = [1];
  const alias: number[] = f;
  f[0] = grow(alias);
  assert(f.length === 2 && f[0] === 7, "growth through an alias");

  // The RHS writing the very slot the statement writes: the outer store wins.
  const g: number[] = [1, 2, 3];
  g[1] = writeSlotOne(g);
  assert(g[1] === 11, "the outer store lands after the RHS's own write");

  // A throwing RHS stores nothing.
  const h: number[] = [1];
  let boomed = false;
  try {
    h[0] = boom();
  } catch (e2) {
    boomed = true;
  }
  assert(boomed && h[0] === 1, "a throwing RHS leaves the element alone");

  // The *index* expression can grow the array too, which only the bounds check
  // reading the current length makes legal.
  const j: number[] = [1];
  j[growTo(j, 3) - 2] = 42;
  assert(j.length === 3 && j[1] === 42, "the index expression grew the array");

  // A shrinking RHS must still throw: the check sees the new, shorter length.
  const d: number[] = [1, 2, 3];
  let threw = false;
  try {
    d[2] = shrink(d);
  } catch (e) {
    threw = true;
  }
  assert(threw, "an index past the shrunk length throws");
  assert(d.length === 2, "the pop landed");
}

function shrink(a: number[]): number {
  a.pop();
  return 5;
}

function unshift(a: number[]): number {
  a.unshift(0);
  return 8;
}

function writeSlotOne(a: number[]): number {
  a[1] = 99;
  return 11;
}

function boom(): number {
  throw new Error("boom");
}

function growTo(a: number[], n: number): number {
  while (a.length < n) {
    a.push(0);
  }
  return n;
}
