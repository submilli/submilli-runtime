// A `for` statement's `let` head is a fresh binding each iteration (ECMA-262
// §14.7.4.4 CreatePerIterationEnvironment), so a closure made in one pass keeps
// that pass's value. One slot for the loop's lifetime would make every closure
// read the value the loop exited with.
function main(): void {
  const basic: Array<() => number> = [];
  for (let i = 0; i < 3; i = i + 1) {
    basic.push(() => i);
  }
  assert(join(basic) === "0,1,2", "each closure keeps its own iteration's binding");

  // No update clause: the copy still happens per pass, after the body's own write.
  const noUpdate: Array<() => number> = [];
  for (let i = 0; i < 3; ) {
    noUpdate.push(() => i);
    i = i + 1;
  }
  assert(join(noUpdate) === "1,2,3", "the body's write lands before the copy");

  // `continue` goes through the copy on its way to the update.
  const skipped: Array<() => number> = [];
  for (let i = 0; i < 6; i = i + 1) {
    if (i % 2 === 0) {
      continue;
    }
    skipped.push(() => i);
  }
  assert(join(skipped) === "1,3,5", "continue does not share a binding across passes");

  // A closure that *writes* the binding writes its own iteration's copy.
  const writers: Array<() => number> = [];
  for (let i = 0; i < 3; i = i + 1) {
    writers.push(() => {
      i = i + 10;
      return i;
    });
  }
  assert(join(writers) === "10,11,12", "a closure's write stays in its own copy");

  // Nested loops each get their own per-iteration binding.
  const nested: Array<() => number> = [];
  for (let i = 0; i < 3; i = i + 1) {
    for (let j = 0; j < 2; j = j + 1) {
      nested.push(() => i * 10 + j);
    }
  }
  assert(join(nested) === "0,1,10,11,20,21", "inner and outer heads both copy");

  // `break` stops before the next copy; the closures already made are unaffected.
  const broken: Array<() => number> = [];
  for (let i = 0; i < 3; i = i + 1) {
    broken.push(() => i);
    if (i === 1) {
      break;
    }
  }
  assert(join(broken) === "0,1", "break leaves earlier copies intact");

  // A write in the body before the closure is visible to that closure.
  const selfWrite: Array<() => number> = [];
  for (let i = 0; i < 3; i = i + 1) {
    i = i + 0;
    selfWrite.push(() => i);
  }
  assert(join(selfWrite) === "0,1,2", "a body write to the head is seen by the closure");

  // Unwinding a `try`/`finally` on the continue path keeps the copy in order.
  const unwound: Array<() => number> = [];
  for (let i = 0; i < 3; i = i + 1) {
    try {
      unwound.push(() => i);
      continue;
    } finally {
      // nothing
    }
  }
  assert(join(unwound) === "0,1,2", "continue through a finally still copies");

  // An uncaptured head is unboxed and needs no copy — the loop is unchanged.
  let sum = "";
  for (let i = 0; i < 3; i = i + 1) {
    sum = sum + i.toString();
  }
  assert(sum === "012", "an uncaptured loop variable is untouched");

  // The loop variable's post-loop value is still the one the update produced.
  let last = 0;
  const keep: Array<() => number> = [];
  for (let i = 0; i < 3; i = i + 1) {
    keep.push(() => i);
    last = i;
  }
  assert(last === 2, "the last body pass saw 2");
  assert(join(keep) === "0,1,2", "and each closure still holds its own pass's value");

  // A `for-of` `const` head was already a fresh binding per iteration.
  const forOf: Array<() => number> = [];
  for (const v of [7, 8, 9]) {
    forOf.push(() => v);
  }
  assert(join(forOf) === "7,8,9", "for-of is unaffected");

  // The rebox is emitted only for a head the loop itself declares. A binding
  // declared outside is one cell for the loop's life, as in JS.
  const outside: Array<() => number> = [];
  let oi = 0;
  for (; oi < 3; oi = oi + 1) {
    outside.push(() => oi);
  }
  assert(join(outside) === "3,3,3", "an externally declared head shares one binding");

  // A `const` head cannot be reassigned, so every copy would be identical.
  const constHead: Array<() => number> = [];
  for (const k = 5; constHead.length < 3; ) {
    constHead.push(() => k);
  }
  assert(join(constHead) === "5,5,5", "a const head needs no copy");

  // The head shadows an outer boxed binding of the same name; the rebox must
  // pick the head's slot, not the outer one.
  let i = 100;
  const readsOuter = (): number => i;
  const shadowed: Array<() => number> = [];
  for (let i = 0; i < 3; i = i + 1) {
    shadowed.push(() => i);
  }
  assert(join(shadowed) === "0,1,2", "the inner head is its own binding");
  assert(readsOuter() === 100, "and the outer binding is untouched");

  // A head with no condition still copies.
  const noCond: Array<() => number> = [];
  for (let i = 0; ; i = i + 1) {
    noCond.push(() => i);
    if (i === 2) {
      break;
    }
  }
  assert(join(noCond) === "0,1,2", "a head with no condition still copies");

  // `continue` out of a `switch` arm goes through the copy.
  const fromSwitch: Array<() => number> = [];
  for (let i = 0; i < 4; i = i + 1) {
    switch (i % 2) {
      case 0:
        continue;
      default:
        fromSwitch.push(() => i);
    }
  }
  assert(join(fromSwitch) === "1,3", "continue from a switch case still copies");

  // A throw out of the body leaves the copies already made intact.
  const thrown: Array<() => number> = [];
  try {
    for (let i = 0; i < 5; i = i + 1) {
      thrown.push(() => i);
      if (i === 2) {
        throw new Error("stop");
      }
    }
  } catch (e: Error) {
    // swallowed
  }
  assert(join(thrown) === "0,1,2", "a throw leaves earlier copies intact");

  // Two closure levels deep still reach the same copy.
  const deep: Array<() => number> = [];
  for (let i = 0; i < 3; i = i + 1) {
    deep.push((): number => {
      const inner = (): number => i;
      return inner();
    });
  }
  assert(join(deep) === "0,1,2", "a doubly nested closure sees the same copy");

  headTypesAllRebox();
}

// Every head type needs a registered box: the rebox reads and rebuilds the cell
// through `box_type_idx`, which has no fallback — a head type with no registered
// box is a compile-time panic, not a wrong answer.
function headTypesAllRebox(): void {
  const strs: Array<() => string> = [];
  for (let s = "a"; s.length < 4; s = s + "a") {
    strs.push(() => s);
  }
  assert(joinStr(strs) === "a,aa,aaa", "string head");

  let flips = 0;
  const bools: Array<() => string> = [];
  for (let b = true; flips < 3; b = !b) {
    flips = flips + 1;
    bools.push(() => (b ? "T" : "F"));
  }
  assert(joinStr(bools) === "T,F,T", "boolean head");

  const bigs: Array<() => string> = [];
  let bn = 0;
  for (let g = 0n; bn < 3; g = g + 1n) {
    bn = bn + 1;
    bigs.push(() => g.toString());
  }
  assert(joinStr(bigs) === "0,1,2", "bigint head");

  let pn = 0;
  const objs: Array<() => string> = [];
  for (let p: Pt | null = null; pn < 3; p = { x: pn }) {
    pn = pn + 1;
    objs.push(() => (p === null ? "null" : p.x.toString()));
  }
  assert(joinStr(objs) === "null,1,2", "nullable-object head");

  let cn = 0;
  const insts: Array<() => number> = [];
  for (let bx = new Cell(0); cn < 3; bx = new Cell(cn)) {
    cn = cn + 1;
    insts.push(() => bx.v);
  }
  assert(join(insts) === "0,1,2", "class-instance head");

  let an = 0;
  const arrs: Array<() => number> = [];
  for (let ar: number[] = [0]; an < 3; ar = [an]) {
    an = an + 1;
    arrs.push(() => ar[0]);
  }
  assert(join(arrs) === "0,1,2", "array head");
}

interface Pt {
  x: number;
}

class Cell {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
}

function join(fns: Array<() => number>): string {
  let out = "";
  for (const f of fns) {
    out = out + (out === "" ? "" : ",") + f().toString();
  }
  return out;
}

function joinStr(fns: Array<() => string>): string {
  let out = "";
  for (const f of fns) {
    out = out + (out === "" ? "" : ",") + f();
  }
  return out;
}
