// A symbolic T[K] may instantiate to undefined or a function, just like T.
function checked<T, K extends keyof T>(value: T[K]): boolean {
  return JSON.stringify(value) === undefined;
}

function optional<T, K extends keyof T>(value: T[K]): string | undefined {
  return JSON.stringify(value);
}

function required<T, K extends keyof T>(value: T[K]): string {
  return JSON.stringify(value);
}

function main(): void {
  assert(checked<{ x: undefined }, "x">(undefined));
  assert(optional<{ x: undefined }, "x">(undefined) === undefined);
  assert(checked<{ x: () => number }, "x">(() => 1));
  assert(optional<{ x: () => number }, "x">(() => 1) === undefined);
  assert(required<{ x: number }, "x">(1) === "1");
  assert(required<{ x: null }, "x">(null) === "null");
  assert(required<{ x: { n: number } }, "x">({ n: 1 }) === '{"n":1}');
  let caught = false;
  try {
    required<{ x: undefined }, "x">(undefined);
  } catch (error) {
    assert(error.name === "MissingValueError");
    caught = true;
  }
  assert(caught, "indexed undefined throws in a string place");
  caught = false;
  try {
    required<{ x: () => number }, "x">(() => 1);
  } catch (error) {
    assert(error.name === "MissingValueError");
    caught = true;
  }
  assert(caught, "indexed function throws in a string place");
}
