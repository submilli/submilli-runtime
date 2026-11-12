// SUB-2262: every value with no JSON text throws a catchable
// `MissingValueError` where the call is read as `string`, and the message
// names the kind of value. Node returns `undefined` for each of these.

function noop(): void {}

function id<T>(x: T): T {
  return x;
}

function bounded<T extends () => number>(x: T): string {
  return JSON.stringify(x);
}

function one(): number {
  return 1;
}

function anyText<T>(x: T): string {
  return JSON.stringify(x);
}

function optionalText<T>(x: T | undefined): string {
  return JSON.stringify(x);
}

function mixedText(x: number | (() => number) | undefined): string {
  return JSON.stringify(x);
}

class Holder {
  static make(): number {
    return 1;
  }
}

function thrown(read: () => string): string {
  try {
    return "no throw: " + read();
  } catch (e) {
    return e.name + ": " + e.message;
  }
}

const NO_UNDEFINED_TEXT =
  "MissingValueError: JSON.stringify: `undefined` has no JSON text; check the value first, or use `JSON.stringify(value) ?? …`";
const NO_FUNCTION_TEXT =
  "MissingValueError: JSON.stringify: a function has no JSON text; pass the data instead of the function, or use `JSON.stringify(value) ?? …`";

function main(): void {
  const f = (x: number) => x;
  const u: unknown = f;
  const v: unknown = noop();

  assert(thrown(() => JSON.stringify(u)) === NO_FUNCTION_TEXT, "unknown holding a function");
  assert(thrown(() => JSON.stringify(v)) === NO_UNDEFINED_TEXT, "unknown holding undefined");
  assert(thrown(() => JSON.stringify(noop())) === NO_UNDEFINED_TEXT, "a void value");
  assert(thrown(() => JSON.stringify(f)) === NO_FUNCTION_TEXT, "a value of a function type");
  assert(thrown(() => JSON.stringify(Number)) === NO_FUNCTION_TEXT, "a constructor");
  assert(thrown(() => JSON.stringify(id(f))) === NO_FUNCTION_TEXT, "a type parameter holding a function");
  assert(thrown(() => bounded(one)) === NO_FUNCTION_TEXT, "a type parameter bounded by a function type");
  assert(thrown(() => JSON.stringify(Holder.make)) === NO_FUNCTION_TEXT, "a static method");
  assert(thrown(() => anyText(f)) === NO_FUNCTION_TEXT, "an unconstrained type parameter holding a function");
  assert(thrown(() => anyText(undefined)) === NO_UNDEFINED_TEXT, "an unconstrained type parameter holding undefined");
  assert(thrown(() => optionalText<() => number>(one)) === NO_FUNCTION_TEXT, "T | undefined holding a function");
  assert(thrown(() => optionalText<number>(undefined)) === NO_UNDEFINED_TEXT, "T | undefined holding undefined");
  assert(thrown(() => mixedText(one)) === NO_FUNCTION_TEXT, "a mixed union holding a function");
  assert(thrown(() => mixedText(undefined)) === NO_UNDEFINED_TEXT, "a mixed union holding undefined");
  assert(mixedText(5) === "5", "a mixed union holding a number");

  assert(thrown(() => JSON.stringify(f, null)) === NO_FUNCTION_TEXT, "a function with a null replacer");
  assert(thrown(() => JSON.stringify(f, null, 2)) === NO_FUNCTION_TEXT, "a function with a numeric space");
  assert(thrown(() => JSON.stringify(f, null, "  ")) === NO_FUNCTION_TEXT, "a function with a string space");
  assert(thrown(() => JSON.stringify(v, null, 2)) === NO_UNDEFINED_TEXT, "undefined with a numeric space");
  assert(thrown(() => JSON.stringify(v, null, "  ")) === NO_UNDEFINED_TEXT, "undefined with a string space");

  const held: string | undefined = JSON.stringify(undefined);
  assert(held === undefined, "a string | undefined place reads undefined");
  assert(JSON.stringify(u) === undefined, "a check reads undefined");

  const w: unknown = { a: [1, "x", null, true] };
  assert(JSON.stringify(w) === "{\"a\":[1,\"x\",null,true]}", "unknown holding an object");
  assert(JSON.stringify(w, null, 1) === "{\n \"a\": [\n  1,\n  \"x\",\n  null,\n  true\n ]\n}", "with space");
}
