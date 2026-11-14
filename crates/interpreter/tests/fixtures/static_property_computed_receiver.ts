// A property of a static built-in read from a computed value evaluates the
// value first, as JavaScript does.
let calls = 0;

function nums(): NumberConstructor {
  calls = calls + 1;
  return Number;
}

function same<T>(value: T): T {
  calls = calls + 10;
  return value;
}

function boom(): NumberConstructor {
  throw new Error("boom");
}

function maybe(present: boolean): NumberConstructor | undefined {
  calls = calls + 100;
  return present ? Number : undefined;
}

function main(): void {
  const max = nums().MAX_SAFE_INTEGER;
  assert(calls === 1 && max === 9007199254740991, "the receiver runs once");
  const epsilon = same(Number).EPSILON;
  assert(calls === 11 && epsilon > 0, "a generic call's effects are kept");
  const local = Number;
  assert(Number.MAX_VALUE > 0 && local.MIN_VALUE > 0 && calls === 11, "a name reads as before");
  let message = "";
  try {
    const value = boom().EPSILON;
    message = String(value);
  } catch (e) {
    message = (e as Error).message;
  }
  assert(message === "boom", "a receiver that throws, throws");
  assert(maybe(true)?.EPSILON === Number.EPSILON && calls === 111, "an optional chain reads it");
  assert(maybe(false)?.EPSILON === undefined && calls === 211, "a missing receiver gives undefined");
}
