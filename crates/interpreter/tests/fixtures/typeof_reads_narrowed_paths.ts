// A type query where it runs reads each step of its path narrowed: `x`
// narrowed to `A` has a `p`, and a call's type argument takes the narrowing
// at the call, as tsc has them.
interface A {
  kind: "a";
  p: string;
}
interface B {
  kind: "b";
  q: number;
}

function lengthOfP(x: A | B): number {
  if (x.kind === "a") {
    const p: typeof x.p = x.p;
    return p.length;
  }
  return 0;
}

function id<T>(t: T): T {
  return t;
}

function pick(): string | number {
  return "abc";
}

function main(): void {
  assert(lengthOfP({ kind: "a", p: "ab" }) === 2, "a narrowed root's field");
  let v: string | number = pick();
  if (typeof v === "string") {
    assert(id<typeof v>(v).length === 3, "a call's type argument");
    // A function type's parameters are a signature, which keeps the
    // declared type even inside an annotation that runs.
    const f: (p: typeof v) => number = (p) => 1;
    assert(f(5) === 1, "a function type in an annotation");
  }
  v = 1;
}
