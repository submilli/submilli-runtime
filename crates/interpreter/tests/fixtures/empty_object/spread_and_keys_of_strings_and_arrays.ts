// A string or an array held in `{}` is still a string or an array: spreading
// it copies its indices, and `Object.keys`, `values`, `entries` and `hasOwn`
// see them, as in JavaScript. A number or boolean has no own properties.
function spreadOf<T>(value: T): string {
  if (value === null || value === undefined) {
    return "nullish";
  }
  return String(JSON.stringify({ ...value }));
}

function main(): void {
  const s: {} = "str";
  assert(JSON.stringify({ ...s }) === "{\"0\":\"s\",\"1\":\"t\",\"2\":\"r\"}", "a string spreads its characters");
  const a: {} = [1, 2];
  assert(JSON.stringify({ ...a }) === "{\"0\":1,\"1\":2}", "an array spreads its elements");
  assert(JSON.stringify({ x: 0, ...a }) === "{\"0\":1,\"1\":2,\"x\":0}", "beside other fields");
  const mixed: unknown[] = [[1], "x", null];
  const m: {} = mixed;
  assert(JSON.stringify({ ...m }) === "{\"0\":[1],\"1\":\"x\",\"2\":null}", "elements keep their values");

  const t: {} = "ab";
  assert(JSON.stringify(Object.keys(t)) === "[\"0\",\"1\"]", "keys of a string");
  assert(JSON.stringify(Object.values(t)) === "[\"a\",\"b\"]", "values of a string");
  assert(JSON.stringify(Object.entries(t)) === "[[\"0\",\"a\"],[\"1\",\"b\"]]", "entries of a string");
  assert(JSON.stringify(Object.entries(a)) === "[[\"0\",1],[\"1\",2]]", "entries of an array");
  const empty: {} = [];
  assert(Object.keys(empty).length === 0, "an empty array has no indices");

  assert(Object.hasOwn(t, "1") && !Object.hasOwn(t, "2"), "hasOwn sees a string's indices");
  assert(Object.hasOwn(t, "length") && !Object.hasOwn(t, "01"), "and its length, not a padded index");
  assert(Object.hasOwn(a, "1") && !Object.hasOwn(a, "2"), "hasOwn sees an array's indices");

  const n: {} = 5;
  const b: {} = true;
  assert(JSON.stringify({ ...n, ...b }) === "{}", "a number or boolean copies nothing");
  assert(Object.keys(n).length === 0 && !Object.hasOwn(n, "0"), "and lists nothing");

  assert(spreadOf("hey") === "{\"0\":\"h\",\"1\":\"e\",\"2\":\"y\"}", "a narrowed type parameter spreads a string");
  assert(spreadOf([7]) === "{\"0\":7}", "and an array");
  assert(spreadOf(3) === "{}", "and copies nothing from a number");
}
