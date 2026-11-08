// SUB-2148 with generic constraints: a `?.` that checks a missing receiver
// typed with a constrained type parameter reads it through the constraint
// with room for `undefined`, so the `?.` skips a missing value instead of
// failing the constraint's cast.
interface Named {
  name: string;
  tag(): string;
}

function firstName<T extends Named>(xs: T[]): string {
  return xs[0]?.name ?? "none";
}

function midStep<T extends Named>(rs: Record<string, T>[], k: string): string {
  return rs[0]?.[k]?.name ?? "none";
}

function firstTag<T extends Named>(xs: T[]): string {
  return xs[0]?.tag() ?? "none";
}

function isA<T extends Named>(xs: T[]): boolean {
  return xs[0]?.name === "a";
}

function heldName<T extends Named>(xs: T[]): string {
  const n = xs[0]?.name;
  return n ?? "dflt";
}

type Shape = { kind: "a"; x: number } | { kind: "b"; y: string };

// The `?.` proves the receiver present, so a discriminant test through it
// narrows the bound's members.
function discriminant<T extends Shape>(xs: T[], i: number): string {
  const v = xs[i];
  if (v?.kind === "a") return "x" + v.x;
  if (xs[0]?.kind === "b") return "y" + xs[0].y;
  return "no";
}

function bareName<T extends Named>(xs: T[]): string {
  return xs[0].name;
}

function lookup<R extends Record<string, number>>(r: R, k: string): number {
  return r[k] ?? -1;
}

function dotted<R extends Record<string, number>>(r: R): number {
  return r.foo ?? -1;
}

function nullable<T extends Named | null>(xs: T[]): string {
  return xs[0]?.name ?? "none";
}

function callFirst<F extends () => string>(fs: F[]): string {
  return fs[0]?.() ?? "none";
}

function throwsMissing(f: () => void): boolean {
  try {
    f();
  } catch (e: MissingValueError) {
    return true;
  }
  return false;
}

function mk(n: string): Named {
  return { name: n, tag: () => "t" + n };
}

function main(): void {
  assert(firstName([]) === "none", "a missing element");
  assert(firstName([mk("a")]) === "a", "a present element");
  assert(midStep([], "x") === "none", "a missing base before a step");
  assert(midStep([{}], "x") === "none", "a missing key mid-chain");
  assert(midStep([{ x: mk("a") }], "x") === "a", "a present key mid-chain");
  assert(firstTag([]) === "none", "a method call on a missing element");
  assert(firstTag([mk("a")]) === "ta", "a method call on a present element");
  assert(!isA([]) && isA([mk("a")]), "=== on a chain");
  assert(heldName([]) === "dflt" && heldName([mk("b")]) === "b", "a local holding the chain");
  assert(discriminant([{ kind: "a", x: 1 }], 0) === "x1", "a discriminant through ?.");
  assert(discriminant([{ kind: "b", y: "q" }], 0) === "yq", "a discriminant on a direct read");
  assert(discriminant([], 3) === "no", "a discriminant on a missing element");
  assert(throwsMissing(() => bareName([])), "a read through a constraint throws");
  assert(lookup({ a: 1 }, "b") === -1 && lookup({ a: 1 }, "a") === 1, "a record read through a constraint");
  assert(dotted({}) === -1 && dotted({ foo: 2 }) === 2, "a dotted read through a constraint");
  assert(nullable([null]) === "none" && nullable([]) === "none", "a nullable constraint");
  assert(nullable([mk("n")]) === "n", "a nullable constraint, present");
  assert(callFirst([]) === "none" && callFirst([() => "c"]) === "c", "a function constraint");
}
