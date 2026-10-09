// test262 harness shim. The conformance runner prepends this file to every
// case under cases/, so these helpers are global there. Mapping from the
// test262 API (assert.sameValue and friends can't exist here — functions
// don't carry properties) is documented in README.md.

function show(v: unknown): string {
  if (v === undefined) {
    return "undefined";
  }
  if (v === null) {
    return "null";
  }
  if (typeof v === "string") {
    return "\"" + v + "\"";
  }
  return v.toString();
}

function assertSameValue(actual: unknown, expected: unknown, message: string = ""): void {
  assert(
    Object.is(actual, expected),
    `expected SameValue(${show(actual)}, ${show(expected)}) ${message}`,
  );
}

function assertNotSameValue(actual: unknown, unexpected: unknown, message: string = ""): void {
  assert(
    !Object.is(actual, unexpected),
    `expected not SameValue(${show(actual)}, ${show(unexpected)}) ${message}`,
  );
}

function assertThrows(fn: () => void, message: string = ""): void {
  let threw = false;
  try {
    fn();
  } catch (e: Error) {
    threw = true;
  }
  assert(threw, `expected an Error to be thrown ${message}`);
}

function assertCompareArray<T>(actual: T[], expected: T[], message: string = ""): void {
  assert(
    actual.length === expected.length,
    `compareArray: length ${actual.length} !== ${expected.length} ${message}`,
  );
  for (let i = 0; i < actual.length; i++) {
    assert(
      Object.is(actual[i], expected[i]),
      `compareArray: index ${i}: ${show(actual[i])} !== ${show(expected[i])} ${message}`,
    );
  }
}
