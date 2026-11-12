// Pins the edges of the Object statics the broader fixtures skip: null-receiver
// throws for keys/values/entries/hasOwn, non-object receivers, optional fields
// holding null, result-array independence, and Object.is across mixed types.
// A null or undefined receiver throws a `TypeError`, as ToObject does.

function expectTypeError(f: () => void, label: string): void {
  let threw: boolean = false;
  try {
    f();
  } catch (e: Error) {
    threw = e instanceof TypeError;
  }
  assert(threw, label);
}

function main(): void {
  const n: unknown = null;
  const u: unknown = undefined;
  expectTypeError(() => { Object.keys(n); }, "Object.keys(null) throws a TypeError");
  expectTypeError(() => { Object.values(n); }, "Object.values(null) throws a TypeError");
  expectTypeError(() => { Object.entries(n); }, "Object.entries(null) throws a TypeError");
  expectTypeError(() => { Object.hasOwn(n, "x"); }, "Object.hasOwn(null) throws a TypeError");
  expectTypeError(() => { Object.hasOwn(u, "x"); }, "Object.hasOwn(undefined) throws a TypeError");
  expectTypeError(() => { Object.hasOwn(JSON.parse("null"), "a"); }, "Object.hasOwn of parsed null throws a TypeError");
  assert(Object.hasOwn(JSON.parse("{\"a\":1}"), "a"), "Object.hasOwn of a parsed object stays true");

  const boxed: unknown = 5;
  assert(!Object.hasOwn(boxed, "x"), "non-object values own no fields");
  assert(Object.values(boxed).length === 0, "non-object values have no values");
  assert(Object.entries(boxed).length === 0, "non-object values have no entries");
  assert(Object.entries({}).length === 0, "empty object has no entries");

  const opt: { a: number; b?: string } = { a: 1 };
  const optKeys = Object.keys(opt);
  assert(optKeys.length === 1 && optKeys[0] === "a", "omitted field has no key");
  assert(Object.values(opt).length === 1, "omitted field has no value");
  assert(Object.entries(opt).length === 1, "omitted field has no entry");
  assert(!Object.hasOwn(opt, "b"), "omitted field is not an own property");
  const present: { a: number; b?: string | null } = { a: 1, b: null };
  assert(Object.values(present)[1] === null, "present optional null has a value");
  assert(Object.entries(present)[1][1] === null, "present optional null has an entry");

  const o = { a: 1, b: 2 };
  const values = Object.values(o);
  values[0] = 99;
  assert(o.a === 1, "result array is a copy, not a view of the object");

  assert(!Object.is(1, "1"), "number and string differ");
  assert(!Object.is(o, 1), "object and number differ");
  assert(!Object.is(true, 1), "boolean and number differ");
  assert(!Object.is("", null), "value and null differ");
}
