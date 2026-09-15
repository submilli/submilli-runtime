// The loop-body wrapper the narrowing fixed point installs has to survive every
// lowering shape, not just the one the crash was reported against. Each function
// below reaches a different branch of the desugar: the `for` guard's
// no-condition early return, the plain `while` passthrough, a stack of three
// nested regions spliced into one guard, and a user `Iterator` body.

function tick(i: number, n: number): IteratorResult<number> {
  if (i >= n) {
    const done: IteratorResult<number> = { done: true };
    return done;
  }
  const r: IteratorResult<number> = { done: false, value: i };
  return r;
}

function range(n: number): Iterator<number> {
  let i = 0;
  const it: Iterator<number> = {
    next: (): IteratorResult<number> => {
      const r = tick(i, n);
      i = i + 1;
      return r;
    },
  };
  return it;
}

// An update clause with no condition: the guard hands the body back whole.
function updateNoCondition(x: string | null): string {
  let out = "";
  for (let i = 0; ; i = i + 1) {
    if (i >= 3) {
      break;
    }
    if (x === null) {
      out = out + "z";
      continue;
    }
    out = out + x;
    continue;
  }
  return out;
}

function bareFor(x: string | null): string {
  let out = "";
  let i = 0;
  for (;;) {
    i = i + 1;
    if (i > 2) {
      break;
    }
    if (x === null) {
      out = out + "z";
      continue;
    }
    out = out + x;
    continue;
  }
  return out;
}

// One condition, three narrowed paths — three nested regions, and the innermost
// must stay innermost when the guard splices them. The all-null call matters:
// its body never runs, and a lowering that hoisted the regions above the
// `if (cond) … else break;` guard would cast null to `string` and trap there.
function threeRegions(a: string | null, b: string | null, c: string | null): number {
  let n = 0;
  for (let i = 0; a !== null && b !== null && c !== null; i = i + 1) {
    n = n + a.length + b.length + c.length;
    if (i >= 1) {
      a = null;
    }
  }
  return n;
}

function overIterator(x: string | null): string {
  let out = "";
  for (const v of range(3)) {
    if (x === null) {
      out = out + v.toString();
      continue;
    }
    out = out + x;
    continue;
  }
  return out;
}

function main(): void {
  assert(updateNoCondition("-") === "---", "update clause, no condition, non-null");
  assert(updateNoCondition(null) === "zzz", "update clause, no condition, null");
  assert(bareFor("-") === "--", "for(;;) non-null");
  assert(bareFor(null) === "zz", "for(;;) null");
  assert(threeRegions("ab", "c", "d") === 8, "three nested regions");
  assert(threeRegions(null, "c", "d") === 0, "three regions, first path null");
  assert(overIterator("-") === "---", "user Iterator, non-null");
  assert(overIterator(null) === "012", "user Iterator, null");
}
