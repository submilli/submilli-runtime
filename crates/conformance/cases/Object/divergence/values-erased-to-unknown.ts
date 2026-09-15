// Not a test262 port: pins two documented divergences in the enumeration
// statics. (1) Object.values / entries erase element types to `unknown` —
// heterogeneous fields need narrowing to be used. (2) Non-object receivers
// yield `[]`, including strings: JS enumerates a string's index keys
// (values/primitive-strings.js expects ['a','b','c']; the original lives
// under rejected/Object/values/).

function main(): void {
  const o = { n: 1, s: "two", b: true };

  const values: unknown[] = Object.values(o);
  assertSameValue(values.length, 3, "every field has a value");
  assertSameValue(values[0] as boolean, true, "b survives the erasure");
  assertSameValue(values[1] as number, 1, "n survives the erasure");
  assertSameValue(values[2] as string, "two", "s survives the erasure");

  const entries: [string, unknown][] = Object.entries(o);
  assertSameValue(entries[1][0], "n", "entry key is a plain string");
  assertSameValue(entries[1][1] as number, 1, "entry value needs narrowing");

  assertSameValue(Object.keys("abc").length, 0, "strings have no keys (JS: index keys)");
  assertSameValue(Object.values("abc").length, 0, "strings have no values (JS: chars)");
  assertSameValue(Object.entries("abc").length, 0, "strings have no entries");
}
