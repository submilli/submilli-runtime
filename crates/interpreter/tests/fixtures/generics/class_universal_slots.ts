// The four universal vtable slots on a generic class: JSON encoding through
// `toJson`, a user `toString` override, and structural `equals`/`hash` when an
// instance is used as a Map key.
class Box<T> {
  constructor(public value: T) {}
}

class Named<T> {
  constructor(public value: T) {}
  toString(): string {
    return "Named";
  }
}

function main(): void {
  assert(JSON.stringify(new Box(42)) === "{\"value\":42}", "toJson at a number instantiation");
  assert(JSON.stringify(new Box("hi")) === "{\"value\":\"hi\"}", "toJson at a string instantiation");
  assert(
    JSON.stringify(new Box(new Box(1))) === "{\"value\":{\"value\":1}}",
    "toJson through a nested instantiation",
  );
  assert(new Named(1).toString() === "Named", "user toString override on a generic class");

  const m = new Map<Box<number>, string>();
  const k = new Box(1);
  m.set(k, "one");
  assert(m.get(k) === "one", "generic instance as a Map key");
  assert(m.get(new Box(1)) === "one", "equals/hash slots compare structurally");
}
