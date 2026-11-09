// SUB-2148: a local holding a maybe-missing read stays checked however it is
// used: written in a loop (where it is boxed), updated with `++`, or narrowed
// by a guard, since a narrowing never proves the value present.
interface Box {
  a: number;
}

function maxOf(xs: number[]): number {
  let best = xs[0];
  for (const x of xs) {
    if (x > best) best = x;
  }
  return best;
}

function lastOf(xs: string[]): string {
  let last = xs[0];
  for (let i = 0; i < xs.length; i++) last = xs[i];
  return last;
}

function bump(ns: number[]): number {
  let v = ns[0];
  v++;
  return v;
}

function notQ(xs: string[]): string {
  const v = xs[0];
  if (v === "q") return "q";
  return v;
}

function boxA(boxes: (Box | null)[]): number {
  const v = boxes[0];
  if (v === null) return -1;
  return v.a;
}

function guarded(boxes: (Box | null)[]): number {
  const v = boxes[0];
  return v !== null ? v.a : -1;
}

function reassigned(r: Record<string, string>, use: boolean): string {
  let v = r["a"];
  if (use) v = r["b"];
  return v;
}

function bestScore(boxes: Box[], i: number): number {
  let best: Box | undefined;
  best = boxes[i];
  return best.a;
}

function bumpNarrowed(xs: (number | null)[]): number {
  let v = xs[0];
  if (v !== null) v += 1;
  return v ?? 0;
}

function longRead(xs: number[]): number {
  const v = xs[
    3
  ];
  return v;
}

function message(read: () => string): string {
  try {
    return "=" + read();
  } catch (e: MissingValueError) {
    return e.message;
  }
}

function main(): void {
  assert(maxOf([3, 9, 4]) === 9, "a present maximum");
  assert(message(() => String(maxOf([]))) === "`best` is missing: `xs[0]` on line 9 found no value", "written in a for...of loop");
  assert(message(() => lastOf([])) === "`last` is missing: a read assigned to it found no value", "written in a for loop");
  assert(bump([1]) === 2, "a present ++");
  assert(message(() => String(bump([]))).startsWith("`v` is missing"), "++ reads the value");
  assert(notQ(["x"]) === "x" && notQ(["q"]) === "q", "a present narrowed read");
  assert(message(() => notQ([])).startsWith("`v` is missing"), "narrowed by !== a value");
  assert(boxA([{ a: 1 }]) === 1 && boxA([null]) === -1, "a present narrowed object");
  assert(message(() => String(boxA([]))).startsWith("`v` is missing"), "narrowed by === null");
  assert(message(() => String(guarded([]))).startsWith("`v` is missing"), "narrowed in ?:");
  assert(bestScore([{ a: 4 }], 0) === 4, "a `| undefined` local narrows to its value, as in tsc");
  assert(
    message(() => String(bestScore([], 0))) === "`best` is missing: a read assigned to it found no value",
    "a `| undefined` local narrowed to `Box` checks the value",
  );
  assert(bumpNarrowed([1]) === 2, "a present compound assignment after a narrowing");
  assert(message(() => String(bumpNarrowed([]))).startsWith("`v` is missing"), "a compound assignment after a narrowing reads the value");
  assert(message(() => String(longRead([]))) === "`v` is missing: the read on line 64 found no value", "a read too long to quote keeps its line");
  const r: Record<string, string> = { a: "A" };
  assert(reassigned(r, false) === "A", "the declaration's read");
  assert(
    message(() => reassigned(r, true)) === "`v` is missing: a read assigned to it found no value",
    "a reassigned read is not named after the declaration's",
  );
}
