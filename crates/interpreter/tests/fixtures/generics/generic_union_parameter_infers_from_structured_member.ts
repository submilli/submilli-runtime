// A parameter that is a bare type parameter beside a member naming it inside
// (`T | T[]`, `A | Box<A>`) infers from the structured member first, as in
// tsc, where the bare type parameter takes the whole argument only at a lower
// priority: `{ v: m }` for `A | Box<A>` binds `A` to `Mode`, not to the
// object.
type Mode = "on" | "off";

interface Box<T> {
  v: T;
}

function oneOrMany<T>(one: T | T[], fallback: T): T | T[] {
  return one;
}

function objOr<A>(a: A | Box<A>, b: A): A {
  return b;
}

function main(): void {
  let m: Mode = "on";
  if (m.length > 5) {
    m = "off";
  }
  const viaUnion = oneOrMany(m, "on");
  const viaArray = oneOrMany([m], "off");
  const viaBox = objOr({ v: m }, "on");
  const plain = objOr(m, "off");
  const boxed: Box<number> = { v: 1 };
  const fromBox = objOr(boxed, 2);
  assert(viaUnion === "on" && viaBox === "on" && plain === "off", "a literal-union argument");
  assert(Array.isArray(viaArray) && fromBox === 2, "an array and an interface argument");
}
