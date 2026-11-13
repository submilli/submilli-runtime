// An object type can have an index signature per key domain, each with its own
// value (SUB-2110). A read takes tsc's applicable signature.
type Both = { [k: string]: string | number; [i: number]: number };
type Disjoint = { [k: `a${string}`]: number; [k: `b${string}`]: string };
type Nested = { [k: `a${string}`]: number | string; [k: `ab${string}`]: number };
type SameValue = { [k: `a${string}`]: number; [k: `b${string}`]: number };
type ReadonlyMix = { readonly [k: `a${string}`]: number; [k: `ab${string}`]: number };
interface Wide { [k: `b${string}`]: string }
interface Combined extends Base, Wide {
  [i: number]: boolean;
  [s: string]: number | string | boolean;
}
// Recursive types whose signatures nest relate without looping.
interface Node1 { [k: `a${string}`]: Node1; [k: `ab${string}`]: Node2 }
interface Node2 { [k: `a${string}`]: Node1; [k: `ab${string}`]: Node2; x?: number }
// Disjoint from a number's name, by its first or last character.
type NumberBeside = { [i: number]: boolean; [k: `${string}px`]: number };
type NumberPattern = { [k: number]: string; [k: `${number}`]: string; [k: `a${string}`]: number };
// Keys that overlap without nesting, but values that do: the narrower value.
interface Prefixed { [k: `a${string}`]: number }
interface Fitting extends Prefixed { [k: `${string}z`]: number | string }
// No number text ends in "Id" or "Code" (a hex letter only ends `0x…`).
type Columns = { [k: `${number}`]: boolean; [k: `${string}Id`]: string; [k: `${string}Code`]: number };
// Overlapping keys whose values intersect exactly: a literal union within
// `number`, and the same object written through an alias and out.
type Point2 = { x: number };
type ExactLiterals = { [k: `a${string}`]: 1 | 2; [k: `${string}z`]: number };
type ExactObjects = { [k: `p${string}`]: Point2; [k: `${string}q`]: { x: number } | undefined };
interface Base { [k: `a${string}`]: number }
interface Derived extends Base { [k: `b${string}`]: string; name: string }

function readA(o: Disjoint, k: `a${string}`): number {
  return o[k];
}

// Each member of a key union writes through its own signature.
function bump(o: Record<string, number>, k: string | number): void {
  o[k] += 1;
}

function setEither(o: SameValue, k: `a${string}` | `b${string}`): void {
  o[k] = 7;
}

// A key type parameter bounded by a union reads each member's signature.
function readKey<K extends keyof SameValue>(o: SameValue, k: K): number {
  return o[k];
}

function values<T>(r: Record<string, T>): T[] {
  const out: T[] = [];
  for (const key in r) {
    out.push(r[key]);
  }
  return out;
}

// A bound whose nested signatures read the parameter itself relates without
// looping: reading `T["abz"]` intersects values that read it again (the
// re-entry bound of `TypeLimits::enter_intersection`).
function selfBound<T extends { [k: `a${string}`]: { v: T["abz"] | 1 }; [k: `ab${string}`]: { v: T["abz"] } }>(
  u: T["abz"],
): boolean {
  const n: { v: unknown } = u;
  return n === undefined;
}

function main(): void {
  const both: Both = { x: "s", 1: 2 };
  const n: number = both[1];
  const s: string | number = both["x"];
  assert(n === 2 && s === "s", "number and string signatures");

  const disjoint: Disjoint = { a1: 1, b1: "s" };
  const a: number = disjoint.a1;
  const b: string = disjoint["b1"];
  disjoint.a2 = 5;
  disjoint["b2"] = "t";
  assert(a === 1 && b === "s" && readA(disjoint, "a2") === 5, "disjoint domains");
  assert(disjoint.a2 === 5 && disjoint.b2 === "t", "writes land");

  // Both signatures apply to `ab1`; the narrower value is the intersection.
  const nested: Nested = { a1: "x", ab1: 2 };
  const ab: number = nested.ab1;
  assert(ab === 2, "nested domains read the intersection");

  const counts: Record<string, number> = { b1: 1 };
  bump(counts, "b1");
  const same: SameValue = {};
  setEither(same, "b2");
  assert(counts.b1 === 2 && same.b2 === 7, "a union key writes through each signature");

  // A write is refused only where every applicable signature is readonly.
  const mix: ReadonlyMix = { a1: 1 };
  mix.ab1 = 2;
  assert(mix.ab1 === 2, "a writable narrower signature takes the write");

  // A target signature relates only to the source signature that applies to
  // its key, so a wider `string` signature beside it doesn't count.
  const narrowOnly: { [i: number]: number } = both;
  const combined: Combined = { a1: 1, b1: "x", 2: true };
  const base: Base = combined;
  assert(narrowOnly[1] === 2 && base.a1 === 1, "assignable to a narrower domain");

  const keyed: SameValue = { a1: 1, b1: 2 };
  assert(readKey(keyed, "b1") === 2, "a key parameter bounded by a union");

  // `T` takes every value the argument's signatures read.
  const all = values(disjoint);
  assert(all.length === 4, "inference from several signatures");

  const node2: Node2 = {};
  const node1: Node1 = node2;
  const back: Node2 = node1;
  assert(back === node2, "recursive nested signatures relate");

  const beside: NumberBeside = { 1: true, wpx: 2 };
  const pattern: NumberPattern = { 1: "one", a1: 2 };
  const fitting: Fitting = { a1: 1, bz: "s", az: 3 };
  const fitted: number = fitting.az;
  assert(beside[1] && beside.wpx === 2 && pattern[1] === "one" && fitted === 3, "disjoint and fitting keys");

  const columns: Columns = { userId: "u", zipCode: 1, "7": true };
  assert(columns.userId === "u" && columns.zipCode === 1 && columns["7"], "number text beside word suffixes");
  const two: 1 | 2 = 2;
  const literals: ExactLiterals = { az: two };
  const objects: ExactObjects = { pq: { x: 3 } };
  const narrowed: 1 | 2 = literals.az;
  const point: Point2 = objects.pq;
  assert(narrowed === 2 && point.x === 3, "exact intersections read as the narrower value");

  const derived: Derived = { name: "n", a1: 1, b1: "q" };
  const inherited: number = derived.a1;
  const own: string = derived.b2 ?? "none";
  assert(inherited === 1 && own === "none", "an interface adds a domain to its base's");

  // `Object.assign` gives tsc's intersection of its sources.
  const ra: Record<`a${string}`, number> = { a1: 1 };
  const rb: Record<`b${string}`, string> = { b1: "x" };
  const merged = Object.assign({}, ra, rb);
  const m1: number | undefined = merged.a1;
  const m2: string | undefined = merged.b1;
  assert(m1 === 1 && m2 === "x", "Object.assign keeps both signatures");

  const wide: { [k: string]: number | string } = disjoint;
  assert(Object.keys(wide).length === 4, "assignable to a wider signature");
  const kinds = Object.values(disjoint).map((v) => typeof v);
  kinds.sort();
  assert(kinds.join() === "number,number,string,string", "values list every signature's");

  const cast = JSON.parse('{"a5":5,"b5":"five"}') as Disjoint;
  assert(cast.a5 === 5 && cast.b5 === "five", "a cast checks each signature");
  let threw = false;
  try {
    const bad = JSON.parse('{"b1":2}') as Disjoint;
    assert(bad.b1 === "never", "unreachable");
  } catch (e) {
    threw = e instanceof TypeError;
  }
  assert(threw, "a cast rejects a value its matching signature does not hold");
  let bothThrew = false;
  try {
    const bad = JSON.parse('{"1":"one"}') as Both;
    assert(bad[1] === -1, "unreachable");
  } catch (e) {
    bothThrew = e instanceof TypeError;
  }
  assert(bothThrew, "a numeric name is checked against the number signature too");
}
