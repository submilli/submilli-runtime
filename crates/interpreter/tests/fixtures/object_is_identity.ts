// `Object.is` is SameValue: two objects, arrays or functions are the same
// value only as one reference. Strings, booleans and bigints compare by value.
class Point {
  constructor(public x: number) {}
}

function same<T>(value: T): T {
  return value;
}

function main(): void {
  const a: unknown = JSON.parse("{\"a\":1}");
  const b: unknown = JSON.parse("{\"a\":1}");
  assert(!Object.is(a, b), "two parsed objects with the same content");
  assert(Object.is(a, a), "one parsed object");
  assert(!Object.is(JSON.parse("[1,2]"), JSON.parse("[1,2]")), "two parsed arrays");
  assert(!Object.is(JSON.parse("{}"), JSON.parse("{}")), "two empty objects");

  const p = new Point(1);
  assert(Object.is(p, p) && !Object.is(p, new Point(1)), "class instances");
  const xs = [1, 2];
  assert(Object.is(xs, xs) && !Object.is(xs, [1, 2]), "typed arrays");
  const o = { x: 1 };
  assert(Object.is(same(o), o) && !Object.is(o, { x: 1 }), "object literals");
  const f = (n: number): number => n;
  assert(Object.is(f, f) && !Object.is(f, (n: number): number => n), "functions");
  const adapted: (n: number, extra: number) => number = f;
  assert(Object.is(adapted, f) && Object.is(f, adapted), "a function adapted to another signature");
  const m = new Map<string, number>();
  assert(Object.is(m, m) && !Object.is(m, new Map<string, number>()), "maps");

  assert(Object.is("ab", "a" + "b"), "strings compare by value");
  assert(Object.is(true, 1 < 2), "booleans compare by value");
  assert(Object.is(10n, 5n * 2n), "bigints compare by value");
  assert(Object.is(NaN, NaN) && !Object.is(0, -0), "numbers follow SameValue");
  assert(Object.is(console, console) && Object.is(Number, Number), "static bindings");
  assert(Object.is(null, null) && !Object.is(null, undefined), "null and undefined");
  assert(!Object.is("1", 1), "a string is not a number");
}
