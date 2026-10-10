// A bare type parameter in a union parameter has the lowest inference
// priority, as in tsc. An argument identical to a member that names no type
// parameter leaves the bare one only a fallback, so a later argument decides
// it; every other member still infers from that argument; and a literal
// matching a literal member stays that literal.
class Box<A> {
  constructor(public v: A) {}
}

function pair<T>(a: T | Box<number>, c: T): T[] {
  return [c];
}

function inner<T, U>(
  a: T | Box<number>,
  b: T | Box<U> | Box<number>,
  cb: (x: U) => U,
): U | null {
  return null;
}

function either<T, U>(a: T | U | "x"): U | null {
  return null;
}

function main(): void {
  const pairs = pair(new Box(1), "s");
  const strings: string[] = pairs;
  const inferred = inner(new Box(true), new Box(1), (x) => x + 1);
  const n: number | null = inferred;
  const literal = either("x");
  const x: "x" | null = literal;
  assert(strings[0] === "s" && n === null && x === null, "naked priority");
}
