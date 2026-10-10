// An object-literal receiver resolves the prelude `Object` interface's members
// through `?.` the way it does through `.`: the literal's own field map is not
// the whole membership list, so a miss on it is not yet a miss on the receiver.

class Klass {
  a: number = 2;
  toString(): string {
    return "klass";
  }
}

function main(): void {
  const o: { a: number } | null = { a: 1 };
  assert(o?.toJson() === '{"a":1}', "a prelude method called through `?.`");
  assert(o?.toString() === "[object Object]", "another prelude method");
  assert(o?.a === 1, "the literal's own field still wins");

  const none: { a: number } | null = null as { a: number } | null;
  assert(none?.toJson() === undefined, "a null receiver short-circuits before dispatch");

  // A class receiver resolves prelude members the same way, including one it
  // overrides.
  const k: Klass | null = new Klass();
  assert(k?.toJson() === '{"a":2}', "a prelude method on a class receiver");
  assert(k?.toString() === "klass", "an overridden prelude method");

  // A prelude member deeper in the chain, and off an array element.
  const nested: { inner: { a: number } } | null = { inner: { a: 3 } };
  assert(nested?.inner.toJson() === '{"a":3}', "a prelude member at step 2");
  const arr: ({ a: number } | null)[] = [{ a: 4 }];
  assert(arr[0]?.toJson() === '{"a":4}', "a prelude member off an array element");
}
