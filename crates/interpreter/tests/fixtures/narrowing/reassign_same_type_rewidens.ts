// The runtime half of the same-type-reassignment rule: once the guard is dead
// the binding reads as its declared type, so the value seen is the one actually
// stored — not the pre-assignment shadow. Re-narrowing after the write is what
// gets the narrowed view back.
function give(n: number | null): number | null {
  return n;
}

let g: number | null = 1;
let gs: string | null = "a";

function main(): void {
  let v: number | null = 1;
  let w: number | null = null;
  if (v !== null) {
    v = w;
    assert(v === null, "the read sees the stored value, not the stale shadow");
  }
  assert(v === null, "and after the block too");

  // Re-narrowing after the write restores the narrowed view.
  let u: number | null = 1;
  let three: number | null = 3;
  let seen = 0;
  if (u !== null) {
    u = three;
    if (u !== null) {
      seen = -u;
    }
  }
  assert(seen === -3, "re-narrowing after the write reads the new value");

  // A branch-local write kills the enclosing guard at the join.
  let b: number | null = 1;
  let out = "";
  if (b !== null) {
    if (b > 0) {
      b = w;
    }
    out = b === null ? "null" : "num";
  }
  assert(out === "null", "the join sees the branch's write");

  // Same rule on a module-level `let`.
  g = 1;
  if (g !== null) {
    g = give(null);
    assert(g === null, "global read sees the stored value");
  }

  // A guard with no write in it still narrows.
  g = 7;
  if (g !== null) {
    assert(-g === -7, "an unwritten guard is untouched");
  }

  // A write that *narrows* installs, on a module-level `let` as on a local, so
  // the reads after it see the value written. A global keeps no shadow — the
  // read goes back to the global itself — which is what makes this agree with
  // the value actually stored.
  g = 1;
  if (g !== null) {
    g = g + 1;
    assert(g === 2, "the in-guard read sees the write");
    assert(-g === -2, "and it is narrowed, not just stored");
  }
  assert(g === 2, "the write landed");

  gs = "a";
  if (gs !== null) {
    gs += "b";
    assert(gs.length === 2, "a compound write to a global narrows too");
  }

  // The writes that must NOT kill a guard.
  assert(writesOtherBinding(3, 4) === -3, "writing `b` leaves `a`'s guard alone");
  assert(writesInnerShadow("abc") === 3, "a shadowed name is a different binding");

  // A write belongs to the branch that made it. The `else` arm never ran the
  // write, so its guard is still good — the kill covers what the writing region
  // can see and goes away with it. TypeScript accepts all three of these.
  assert(siblingArm(null, false) === -1, "the else arm never ran the write");
  assert(branchThatLeaves(1) === -1, "a returning branch never reaches the read");
  assert(siblingField({ v: 1 }, null, false) === -1, "same, through a field path");

  // A compound write to a narrowed local keeps the view and reads the new value.
  let c: number | null = 1;
  if (c !== null) {
    c += 1;
    assert(c === 2 && -c === -2, "a local compound write updates the shadow");
  }
}

function siblingArm(w: number | null, k: boolean): number {
  let v: number | null = 1;
  if (v !== null) {
    if (k) {
      v = w;
    } else {
      return -v;
    }
  }
  return 0;
}

function branchThatLeaves(v: number | null): number {
  let w: number | null = null;
  if (v !== null) {
    if (v > 100) {
      v = w;
      return 0;
    }
    return -v;
  }
  return 0;
}

interface Cell {
  v: number | null;
}

function siblingField(o: Cell, w: number | null, k: boolean): number {
  if (o.v !== null) {
    if (k) {
      o.v = w;
    } else {
      return -o.v;
    }
  }
  return 0;
}

function writesOtherBinding(a: number | null, b: number | null): number {
  if (a !== null) {
    b = null;
    return -a;
  }
  return 0;
}

function writesInnerShadow(v: string | null): number {
  if (v !== null) {
    {
      let v: string | null = null;
      v = null;
    }
    return v.length;
  }
  return 0;
}
