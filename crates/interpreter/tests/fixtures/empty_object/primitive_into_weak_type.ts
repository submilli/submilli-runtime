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

function describeMaybe(value: {} | undefined): string {
  return value === undefined ? "none" : maybeDescribe(value);
}

function readA(value: {}): string {
  const o: Options = value;
  return String(o.a);
}

function passThrough(value: {} | undefined): Options | undefined {
  return value;
}

function narrowedGeneric<T>(value: T): string {
  if (value !== null && value !== undefined) {
    const o: Options = value;
    return String(o.a);
  }
  return "nullish";
}

class Holder {
  a: number = 3;
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
  assert(describeMaybe(n) === "undefined/undefined", "a `{}` that may be undefined");
  assert(describeMaybe(undefined) === "none", "and is");
  assert(passThrough(undefined) === undefined, "undefined stays undefined");
  assert(passThrough(n)?.a === undefined, "beside a converted number");
  assert(narrowedGeneric(5) === "undefined", "a type parameter narrowed to non-nullish");
  assert(narrowedGeneric({ a: 4 }) === "4", "holding an object");
  assert((n as Options).a === undefined, "a cast");
  assert((s as { [k: string]: unknown })["0"] === "s", "a cast to an index signature");
  assert(readA(new Map<string, number>()) === "undefined", "a Map has no own fields");
  assert(readA(new Set<number>()) === "undefined", "nor a Set");
  assert(readA(() => 1) === "undefined", "nor a function");
  assert(readA(new Holder()) === "3", "a class instance keeps its fields");

  const empty = {};
  assert(describe(empty) === "undefined/undefined", "an empty object literal");
}
