// A mutually-recursive alias reaches its own union as a bare back-edge — a name
// with no body, which `Type::union` cannot flatten and `peel` cannot see
// through. Every probe that asks "can this hold null" has to resolve the name
// (typechecker) or treat it as nullable (codegen, which has no type table at the
// point it asks), or a type that spells `null` one name away is lowered into a
// non-null slot and read back with a `ref.as_non_null`.

type J = number | null | Wrap[];
type Wrap = J | string;

// The same shape with no `string` arm, so `Wrap2` aliases the recursive union
// directly rather than wrapping it.
type J2 = number | null | Wrap2[];
type Wrap2 = J2;

// The self-recursive control, which never had a back-edge in the *member*
// position and has always worked.
type Json = string | number | boolean | null | Json[];

// A ring of three, so the back-edge is two hops from where it is asked about.
type R1 = number | null | R2[];
type R2 = R1 | string;
type R3 = R2 | boolean;

// A generic recursive alias.
type Tree<T> = T | null | Tree<T>[];

interface RBox {
  j: R1;
}

interface ChainNode {
  j: R2;
  label: string;
}

class RHolder {
  v: R2 = null;
  m: Map<string, R2> = new Map<string, R2>();
}

class RGeneric<T> {
  v: T;
  constructor(v: T) {
    this.v = v;
  }
}

function nullOf(): Wrap {
  return null;
}

function takeRing(x: R2): string {
  return JSON.stringify(x);
}

function giveRing(): R3 {
  return null;
}

function main(): void {
  const w: Wrap = null;
  assert(w === null, "a mutually-recursive alias holds null");
  assert(JSON.stringify(w) === "null", "and serializes as null");

  // No `??`-on-a-non-nullable warning: the left side really can be null.
  assert((w ?? 5) === 5, "`??` takes the right side");
  assert((nullOf() ?? 7) === 7, "the same through a call");

  const some: Wrap = 3;
  assert((some ?? 5) === 3, "`??` keeps a non-null left side");
  assert(JSON.stringify(some) === "3", "a number member serializes");

  const s: Wrap = "text";
  assert(JSON.stringify(s) === '"text"', "a string member serializes");

  const w2: Wrap2 = null;
  assert(w2 === null, "an alias-of-a-recursive-alias holds null");
  assert(JSON.stringify(w2) === "null", "and serializes as null");

  const nested: Wrap[] = [null, 1, "s"];
  assert(JSON.stringify(nested) === '[null,1,"s"]', "a nested array of the alias");

  const r3: R3 = null;
  assert(r3 === null, "a three-alias ring holds null");
  assert((r3 ?? 9) === 9, "and `??` takes the right side — no bogus non-nullable warning");
  assert(JSON.stringify(r3) === "null", "and it serializes");

  assert(takeRing(null) === "null", "a recursive alias as a parameter");
  assert(giveRing() === null, "a recursive alias as a return type");
  assert((giveRing() ?? 7) === 7, "`??` on a call returning a recursive alias");

  const rbox: RBox = { j: null };
  assert(rbox.j === null, "a recursive alias in an object field");
  assert(JSON.stringify(rbox) === '{"j":null}', "and the object serializes");

  const rh = new RHolder();
  rh.m.set("a", null);
  rh.m.set("b", 2);
  assert(rh.v === null, "a recursive alias as a class field");
  assert(rh.m.get("a") === null, "a recursive alias as a Map value, null arm");
  assert(rh.m.get("b") === 2, "and its non-null arm");
  assert(new RGeneric<R2>(null).v === null, "a recursive alias as a generic argument");

  const t: Tree<string> = null;
  assert(t === null, "a generic recursive alias holds null");
  assert((t ?? "d") === "d", "and `??` takes the right side");
  const tn: Tree<string> = ["a", null, ["b"]];
  assert(JSON.stringify(tn) === '["a",null,["b"]]', "a nested generic recursive alias serializes");

  const cn: ChainNode | null = { j: null, label: "l" };
  assert(cn?.j === null, "a recursive-alias field through an optional chain");
  assert((cn?.j ?? 5) === 5, "and into `??`");
  assert(JSON.stringify(cn?.j) === "null", "and it stringifies");

  const u: unknown = null;
  assert((u as R2) === null, "`as` into a recursive alias, null value");
  const x1: R2 = null as R2;
  const x2: R2 = null as R2;
  const x3: R2 = "s" as R2;
  assert(x1 === x2, "`===` between two null recursive-alias values");
  assert(!(x1 === x3), "and between a null and a non-null one");

  const j: Json = null;
  assert(JSON.stringify(j) === "null", "the self-recursive control still works");
  const jn: Json = [null, 1, "a", true];
  assert(JSON.stringify(jn) === '[null,1,"a",true]', "and its array form");
}
