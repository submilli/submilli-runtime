// Pins the edges of the Object statics the broader fixtures skip: null-receiver
// throws for values/entries/hasOwn, non-object receivers, optional fields
// holding null, result-array independence, and Object.is across mixed types.

function expectNullThrow(f: () => void, label: string): void {
  let threw: boolean = false;
  try {
    f();
  } catch (e: Error) {
    threw = true;
  }
  assert(threw, label);
}

function main(): void {
  const n: unknown = null;
  expectNullThrow(() => { Object.values(n); }, "Object.values(null) throws a catchable Error");
  expectNullThrow(() => { Object.entries(n); }, "Object.entries(null) throws a catchable Error");
  expectNullThrow(() => { Object.hasOwn(n, "x"); }, "Object.hasOwn(null) throws a catchable Error");

  const boxed: unknown = 5;
  assert(!Object.hasOwn(boxed, "x"), "non-object values own no fields");
  assert(Object.values(boxed).length === 0, "non-object values have no values");
  assert(Object.entries(boxed).length === 0, "non-object values have no entries");
  assert(Object.entries({}).length === 0, "empty object has no entries");

  const opt: { a: number; b?: string } = { a: 1 };
  const optKeys = Object.keys(opt);
  assert(optKeys.length === 2, "optional field is part of the shape");
  assert(optKeys[0] === "a" && optKeys[1] === "b", "optional field keeps sorted order");
  const optValues = Object.values(opt);
  assert(optValues[1] === null, "unset optional field surfaces as null");
  const optEntries = Object.entries(opt);
  assert(optEntries[1][0] === "b", "optional field has an entry");
  assert(optEntries[1][1] === null, "entry carries the null");

  const o = { a: 1, b: 2 };
  const values = Object.values(o);
  values[0] = 99;
  assert(o.a === 1, "result array is a copy, not a view of the object");

  assert(!Object.is(1, "1"), "number and string differ");
  assert(!Object.is(o, 1), "object and number differ");
  assert(!Object.is(true, 1), "boolean and number differ");
  assert(!Object.is("", null), "value and null differ");
}
