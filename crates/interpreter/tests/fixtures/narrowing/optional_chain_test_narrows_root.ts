// A test that only a value the chain produced can pass narrows the chain's
// receivers to present, and the chain's result as a plain read.
type Thing = { foo: string | number; bar(): number; baz: object; n: { s: string | null } | null };
class Box {
  constructor(public v: number) {}
}
type Holder = { b: Box | string };

function sum(o: Thing | undefined, v: number, h: Holder | null): number {
  let r = 0;
  if (o?.foo === v) r += o.foo + 1;
  if (o?.["foo"] === v) r += o.bar();
  if (o?.bar() === v) r += o.bar();
  if (o?.foo !== v) {
  } else {
    r += o.foo;
  }
  if (o?.foo === "abc") r += o.foo.length;
  if (typeof o?.foo === "number") r += o.foo;
  if (typeof o?.foo !== "undefined") r += 100;
  if (o?.n?.s === "x") r += o.n.s.length;
  if (h?.b instanceof Box) r += h.b.v;
  return r;
}

function byTypeof(o: Thing | undefined): number {
  switch (typeof o?.foo) {
    case "string":
      return o.foo.length;
    case "number":
      return o.foo + 1;
  }
  return -1;
}

function byCase(o: Thing | undefined): string {
  switch (o?.foo) {
    case "abc":
      return o.foo.toUpperCase();
    case 42:
      return o.foo.toFixed(1);
  }
  return "none";
}

// Equal to a value that may be `null`, the result may still be `null`.
function keepsNull(o: Thing | undefined, v: { s: string | null } | null): boolean {
  if (o?.n === v) return o.n === null;
  return false;
}

type Circle = { kind: "circle"; r: number };
type Square = { kind: "square"; side: number };
type Holds = { shape: Circle | Square; tag: "a" | "b" | number };

// The result narrows as the plain read would, the receivers being present:
// through a discriminant, and to only the literals the chain can produce.
function nested(o: Holds | undefined, k: "a" | "c"): number {
  if (o?.shape.kind === "circle") return o.shape.r;
  if (o?.tag === k) {
    const a: "a" = o.tag;
    return a.length;
  }
  return 0;
}

// A generic result may hold any literal.
function generic<T>(o: { x: T } | undefined): T | undefined {
  if (o?.x === "a") return o.x;
  return undefined;
}

// A label that is a constant names its tags.
const NUMBER = "number";
function byConstantTag(o: Thing | undefined): number {
  switch (typeof o?.foo) {
    case NUMBER:
      return typeof o.foo === "number" ? o.foo + 1 : 0;
  }
  return -1;
}

// A tuple already narrowed still narrows by a position's truthiness.
type Triple = [null, string] | [true, number] | ["s", boolean];
function triple(v: Triple): string {
  if (v[0] === "s") return "s";
  if (!v[0]) return "falsy";
  return String(v[1] + 1);
}

function main(): void {
  assert(sum(undefined, 1, null) === 0);
  assert(sum({ foo: 1, bar: () => 1, baz: {}, n: null }, 1, { b: new Box(5) }) === 111);
  assert(sum({ foo: "abc", bar: () => 7, baz: {}, n: { s: "x" } }, 7, { b: "s" }) === 111);
  assert(byTypeof({ foo: "abcd", bar: () => 0, baz: {}, n: null }) === 4);
  assert(byTypeof({ foo: 41, bar: () => 0, baz: {}, n: null }) === 42);
  assert(byTypeof(undefined) === -1);
  assert(byCase({ foo: "abc", bar: () => 0, baz: {}, n: null }) === "ABC");
  assert(byCase({ foo: 42, bar: () => 0, baz: {}, n: null }) === "42.0");
  assert(byCase(undefined) === "none");
  assert(nested({ shape: { kind: "circle", r: 3 }, tag: 1 }, "a") === 3);
  assert(nested({ shape: { kind: "square", side: 2 }, tag: "a" }, "a") === 1);
  assert(nested(undefined, "a") === 0);
  assert(generic({ x: "a" }) === "a");
  assert(generic(undefined) === undefined);
  assert(byConstantTag({ foo: 1, bar: () => 0, baz: {}, n: null }) === 2);
  assert(byConstantTag(undefined) === -1);
  assert(triple(["s", true]) === "s");
  assert(triple([null, "y"]) === "falsy");
  assert(triple([true, 4]) === "5");
  assert(keepsNull({ foo: 0, bar: () => 0, baz: {}, n: null }, null));
}
