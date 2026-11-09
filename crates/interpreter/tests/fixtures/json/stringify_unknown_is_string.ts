// SUB-2190: `JSON.stringify` is `string` for every argument, as in tsc.
// A value with no JSON text (`undefined` or a function) is a missing read:
// a check, or a place whose type includes `undefined`, reads `undefined`;
// anywhere else the call throws `MissingValueError`. Node returns `undefined`
// in both places. Object, array, string, number, boolean and null are unchanged.

interface FromNumConstructor {
  (v: number): number;
}

function f(): unknown {
  return 1;
}

function asString(value: unknown): string {
  return JSON.stringify(value);
}

function asOptional(value: unknown): string | undefined {
  return JSON.stringify(value);
}

function thrown(read: () => string): string {
  try {
    return read();
  } catch (e) {
    return e.name + ": " + e.message;
  }
}

function holdCheck(): boolean {
  const held = JSON.stringify(undefined);
  return held === undefined;
}

function holdUse(): string {
  const held = JSON.stringify(undefined);
  return held;
}

function invoke(read: () => string): string {
  return read();
}

const NO_TEXT =
  "MissingValueError: `JSON.stringify` has no JSON text for this value (it is `undefined` or a function); use `JSON.stringify(value) ?? …` or check the value first";

function main(): void {
  assert(asString(f()) === "1", "unknown holding a number is a string");
  assert(JSON.stringify(f()) === "1", "the call's own type is string");

  assert(thrown(() => JSON.stringify(undefined)) === NO_TEXT, "undefined in a string place throws");
  assert(thrown(() => JSON.stringify(() => 1)) === NO_TEXT, "a function in a string place throws");
  assert(thrown(() => asString(undefined)) === NO_TEXT, "a string return throws");
  assert(asOptional(undefined) === undefined, "a string | undefined place returns undefined");
  assert(asOptional(() => 1) === undefined, "a function in an optional place returns undefined");
  assert(JSON.stringify(undefined) === undefined, "a check sees no JSON text as undefined");
  assert((JSON.stringify(() => 1) ?? "missing") === "missing", "nullish coalescing takes the fallback");

  assert(holdCheck(), "an unannotated local holds the missing text");
  assert(thrown(holdUse).startsWith("MissingValueError:"), "a use of that local throws");

  assert(JSON.stringify(null) === "null", "null");
  assert(JSON.stringify(true) === "true", "boolean");
  assert(JSON.stringify(false) === "false", "boolean false");
  assert(JSON.stringify(0) === "0", "number");
  assert(JSON.stringify("a") === "\"a\"", "string");
  assert(JSON.stringify([1, null]) === "[1,null]", "array");
  assert(JSON.stringify({ b: 1, a: true }) === "{\"a\":true,\"b\":1}", "object");
  assert(JSON.stringify(Number) === undefined, "a constructor binding is a function");
  const text: string | undefined = JSON.stringify(Number);
  assert(text === undefined, "a constructor binding in an optional place");
  assert(invoke(() => "ok") === "ok", "an unused outer missing value does not break a closure");

  // A user interface whose name ends in Constructor is not a static binding.
  // Number assigned to it is still a closure, and a closure's toJson is "null".
  const asCall: FromNumConstructor = Number;
  assert(JSON.stringify(asCall) === "null", "a call-signature interface is not omitted");
  assert(thrown(() => unionText(asCall)) === NO_TEXT, "a function in an undefined union has no JSON text");
  assert(thrown(() => unionText(undefined)) === NO_TEXT, "undefined in that union throws in a string place");
  assert(unionCheck(asCall), "a check sees a function in that union as undefined");
  assert(unionCheck(undefined), "a check sees undefined in that union");
}

function unionCheck(value: FromNumConstructor | undefined): boolean {
  return JSON.stringify(value) === undefined;
}

function unionText(value: FromNumConstructor | undefined): string {
  return JSON.stringify(value);
}
