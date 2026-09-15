// A narrowing assignment takes a snapshot of the binding in a slot of the
// narrowed type. The next assignment must still write the binding's own slot —
// storing a widened value into the snapshot both loses the write and, when the
// narrowed slot is a non-nullable ref or an unboxed `f64`, is invalid Wasm.

function rewidenString(p: string | null): string | null {
  let x: string | null = p;
  x = "ab";
  x = null;
  return x;
}

function rewidenNumber(p: number | null): number | null {
  let x: number | null = p;
  x = 7;
  x = null;
  return x;
}

function narrowTwiceThenWiden(): string | null {
  let x: string | null = "a";
  x = "ab";
  x = "cde";
  x = null;
  return x;
}

// The second narrowing must not write through the first one's snapshot either:
// the binding's own slot is what a later widening and every non-narrowed read
// resolve to.
function narrowTwiceThenRead(): string {
  let x: string | null = null;
  x = "ab";
  x = "cde";
  return x;
}

function rewidenInsideBranch(cond: boolean, p: string | null): string | null {
  let x: string | null = p;
  x = "ab";
  if (cond) {
    x = null;
  }
  return x;
}

function widenToTheOtherMember(): string | number {
  let x: string | number = 1;
  x = "ab";
  x = 2;
  return x;
}

// Every declared spelling with a distinct Wasm representation for the widened
// slot: a nullable ref, an unboxed `f64`, an `i32`, and the ref-typed builtins.
class C {
  v: number = 1;
}

function rewidenBoolean(): boolean | null {
  let b: boolean | null = null;
  b = true;
  b = null;
  return b;
}

function rewidenBigInt(): bigint | null {
  let g: bigint | null = null;
  g = 5n;
  g = null;
  return g;
}

function rewidenArray(): number[] | null {
  let a: number[] | null = null;
  a = [1];
  a = null;
  return a;
}

function rewidenClass(): C | null {
  let c: C | null = null;
  c = new C();
  c = null;
  return c;
}

function rewidenUnknown(): boolean {
  let u: unknown = 1;
  u = "s";
  u = 2;
  return typeof u === "number";
}

// The widening write can sit in any region the narrowing one can.
function rewidenInWhile(): string | null {
  let x: string | null = null;
  let i: number = 0;
  while (i < 3) {
    x = "a";
    if (i === 1) {
      x = null;
    }
    i = i + 1;
  }
  return x;
}

function rewidenInSwitch(k: number): string | null {
  let x: string | null = "start";
  switch (k) {
    case 0:
      x = "zero";
      x = null;
      break;
    default:
      x = "other";
      break;
  }
  return x;
}

function rewidenInTry(): string | null {
  let x: string | null = null;
  try {
    x = "t";
    x = null;
  } catch (e) {
    x = "c";
  } finally {
    if (x === null) {
      x = "f";
    }
  }
  return x;
}

// An inner `let` of the same name is a different binding: narrowing and
// widening it must not reach the outer one's slot.
function shadowedBinding(): string {
  let x: string | null = "outer";
  {
    let x: string | null = "inner";
    x = null;
    if (x !== null) {
      return "inner leaked";
    }
  }
  x = "outer2";
  return x;
}

// A snapshot is sound only for reads that both *follow* it in the emitted code
// and follow it at run time. A loop back edge separates the two: the read at the
// top of the body runs again after the body's own write, so it has to see the
// storage slot, not the snapshot taken before the loop.
function readAboveTheWrite(): string {
  let x: string | null = null;
  x = "abc";
  let out: string = "";
  let i: number = 0;
  while (i < 2) {
    out = out + x;
    x = "de";
    i = i + 1;
  }
  return out;
}

function readAboveTheWriteDoWhile(): string {
  let y: string | null = null;
  y = "a";
  let out: string = "";
  let n: number = 0;
  do {
    out = out + y;
    y = "Z";
    n = n + 1;
  } while (n < 3);
  return out;
}

function readAboveTheWriteFor(): string {
  let x: string | null = null;
  x = "a";
  let out: string = "";
  for (let i: number = 0; i < 3; i = i + 1) {
    out = out + x;
    x = "b";
  }
  return out;
}

// Same shape with a compound assignment as the loop's write.
function compoundInLoop(): number {
  let x: number | null = null;
  x = 1;
  let sum: number = 0;
  let i: number = 0;
  while (i < 3) {
    sum = sum + x;
    x += 1;
    i = i + 1;
  }
  return sum;
}

// A loop that never runs, and one left by `break` before its write: neither
// executed the assignment the snapshot would have come from.
function zeroTripLoop(): string | null {
  let x: string | null = null;
  x = "pre";
  let i: number = 0;
  while (i < 0) {
    x = "in";
    i = i + 1;
  }
  return x;
}

function brokenOutOfLoop(): string | null {
  let x: string | null = null;
  x = "pre";
  let i: number = 0;
  while (i < 5) {
    if (i === 0) {
      break;
    }
    x = "in";
    i = i + 1;
  }
  return x;
}

// A captured binding lives in a box, not a plain slot, so its write path is the
// one that must find the box rather than a snapshot. The `for`-head case also
// re-boxes per iteration, which is the only shape that reaches that write.
function sv(s: string | null): string {
  return s ?? "N";
}

function capturedForHead(): string {
  const fns: (() => string)[] = [];
  for (let x: string | null = null; fns.length < 3; ) {
    x = "v" + fns.length.toString();
    if (fns.length === 1) {
      x = null;
    }
    fns.push((): string => sv(x));
  }
  let out: string = "";
  for (let i: number = 0; i < 3; i = i + 1) {
    out = out + fns[i]();
  }
  return out;
}

function capturedLet(): string {
  let x: string | null = null;
  const read = (): string => sv(x);
  x = "ab";
  const a = read();
  x = null;
  const b = read();
  x = "cd";
  return a + "|" + b + "|" + read();
}

function boxedParam(p: string | null): string {
  const read = (): string => sv(p);
  p = "ab";
  const a = read();
  p = null;
  return a + "|" + read();
}

function main(): void {
  assert(rewidenString("z") === null, "string rewiden");
  assert(rewidenNumber(3) === null, "number rewiden");
  assert(narrowTwiceThenWiden() === null, "two narrowings then widen");
  assert(narrowTwiceThenRead() === "cde", "second narrowing is the live value");
  assert(rewidenInsideBranch(true, "z") === null, "widened in branch");
  assert(rewidenInsideBranch(false, "z") === "ab", "branch not taken");
  const w = widenToTheOtherMember();
  assert(typeof w === "number" && w === 2, "widened to the other union member");
  assert(rewidenBoolean() === null, "boolean | null");
  assert(rewidenBigInt() === null, "bigint | null");
  assert(rewidenArray() === null, "number[] | null");
  assert(rewidenClass() === null, "C | null");
  assert(rewidenUnknown(), "unknown");
  assert(rewidenInWhile() === "a", "widened inside a while body");
  assert(rewidenInSwitch(0) === null, "widened inside a switch case");
  assert(rewidenInSwitch(1) === "other", "sibling case unaffected");
  assert(rewidenInTry() === "f", "widened inside a try body");
  assert(shadowedBinding() === "outer2", "an inner binding of the same name");
  assert(readAboveTheWrite() === "abcde", "while back edge refreshes the read");
  assert(readAboveTheWriteDoWhile() === "aZZ", "do-while back edge");
  assert(readAboveTheWriteFor() === "abb", "for back edge");
  assert(compoundInLoop() === 6, "compound assignment across a back edge");
  assert(zeroTripLoop() === "pre", "a loop that never ran took no snapshot");
  assert(brokenOutOfLoop() === "pre", "a `break` before the write");
  assert(capturedForHead() === "v0Nv2", "a captured `for`-head binding");
  assert(capturedLet() === "ab|N|cd", "a captured `let`");
  assert(boxedParam("x") === "ab|N", "a boxed parameter");
}
