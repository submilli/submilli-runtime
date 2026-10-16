// `object` holds every value but the primitives, `null` and `undefined`, as in
// TypeScript: objects, arrays, functions and class instances.
class Point {
  constructor(public x: number) {}
}

function keep(value: object): object {
  return value;
}

function kind(x: unknown): string {
  if (typeof x === "object") {
    const objectOrNull: object | null = x;
    if (objectOrNull === null) {
      return "null";
    }
    const narrowed: object = objectOrNull;
    return "k" in narrowed ? "object with k" : "object";
  }
  return typeof x;
}

function fromEmpty(x: {}): string {
  if (typeof x === "object") {
    const narrowed: object = x;
    return narrowed === x ? "object" : "lost";
  }
  return "primitive";
}

function rejectsCast(value: unknown): boolean {
  try {
    const cast = value as object;
    return cast === null;
  } catch (e) {
    return e instanceof TypeError;
  }
}

function spreadAndRest(o: object): number {
  const copy = { ...o, k: 1 };
  const { ...rest } = o;
  return copy.k;
}

function notObjectTag(x: object | null): string {
  if (typeof x !== "object") {
    return "never";
  }
  return x === null ? "null" : "object";
}

function describe(o: object): string {
  return `${o}|${String(o)}`;
}

function main(): void {
  const values: object[] = [{ a: 1 }, [1, 2], () => 1, new Point(1), new Uint8Array(2)];
  assert(values.length === 5, "every non-primitive is an object");
  assert(keep(values[0]) === values[0], "an object passes through unchanged");

  assert(kind({ k: 1 }) === "object with k", "`in` reads a field of an object");
  assert(kind({}) === "object", "an empty object");
  assert(kind(null) === "null", "`typeof null` is \"object\"");
  assert(kind(3) === "number", "a number is not an object");

  assert(fromEmpty({}) === "object", "`{}` narrows to `object`");
  assert(fromEmpty(1) === "primitive", "`{}` may hold a primitive");

  assert(!rejectsCast({}), "casting an object to `object` succeeds");
  assert(!rejectsCast(() => 1), "a function is an object");
  assert(rejectsCast(3), "casting a number to `object` throws");
  assert(rejectsCast("s"), "casting a string to `object` throws");
  assert(rejectsCast(null), "casting `null` to `object` throws");
  assert(rejectsCast(undefined), "casting `undefined` to `object` throws");

  assert(spreadAndRest([1]) === 1, "an `object` spreads and rest-destructures like `{}`");
  assert(notObjectTag({}) === "object", "`typeof` answers \"object\" for an object");
  assert(describe([1, 2]) === "1,2|1,2", "an `object` converts to a string");
  assert(describe({}) === "[object Object]|[object Object]", "a plain object converts too");
}
