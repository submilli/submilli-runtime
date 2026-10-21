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
  assert(keepsNull({ foo: 0, bar: () => 0, baz: {}, n: null }, null));
}
