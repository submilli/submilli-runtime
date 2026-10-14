// A primitive or array held in `{}` may flow into an object type whose
// fields are all optional, or one with an index signature, as in
// TypeScript. Its fields then read as JavaScript reads them from the
// wrapped value: a number has none, a string and an array their indices.
interface Options { a?: number; b?: string }

function describe(o: Options): string {
  return String(o.a) + "/" + String(o.b);
}

function maybeDescribe(o?: Options): string {
  return o === undefined ? "none" : describe(o);
}

function asOptions(value: {}): Options {
  return value;
}

function main(): void {
  const n: {} = 1;
  const literal: { a?: number } = n;
  assert(literal.a === undefined, "a number has no fields");
  const viaInterface: Options = n;
  assert(viaInterface.a === undefined, "nor read through an interface");
  assert(describe(n) === "undefined/undefined", "an argument");
  assert(maybeDescribe(n) === "undefined/undefined", "an optional argument");
  assert(describe(asOptions("x")) === "undefined/undefined", "a returned value");
  const list: Options[] = [n];
  assert(list[0].a === undefined, "an array element");
  const holder: { o: Options } = { o: n };
  assert(holder.o.b === undefined, "a field");
  let reassigned: Options = { a: 2 };
  reassigned = n;
  assert(reassigned.a === undefined, "an assignment");

  const s: {} = "st";
  const byKey: { [k: string]: unknown } = s;
  assert(byKey["1"] === "t", "a string's indices read through an index signature");
  const arr: {} = [1, 2];
  const byIndex: { [k: string]: unknown } = arr;
  assert(byIndex["1"] === 2, "and an array's");

  const object: {} = { a: 5 };
  const same: Options = object;
  assert(same.a === 5 && same === object, "an object stays itself");
  const empty = {};
  assert(describe(empty) === "undefined/undefined", "an empty object literal");
}
