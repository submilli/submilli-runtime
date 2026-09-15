// Not a test262 port: pins our documented divergence from JS property
// enumeration order. test262's keys/values/entries `return-order.js` expect
// integer-keys-then-insertion order; our objects have no insertion order
// (structural types, canonical field layout), so Object.keys / values /
// entries enumerate in canonical sorted key order — the same order JSON
// output uses. The originals live under rejected/Object/.

function main(): void {
  const o = { p3: "p3", p1: "p1", p2: "p2" };

  assertCompareArray(Object.keys(o), ["p1", "p2", "p3"], "keys in sorted order");

  const values = Object.values(o);
  assertSameValue(values.length, 3, "values follow the same order");
  assertSameValue(values[0] as string, "p1", "values[0] is the p1 field");
  assertSameValue(values[2] as string, "p3", "values[2] is the p3 field");

  const entries = Object.entries(o);
  assertSameValue(entries[0][0], "p1", "entries[0] key");
  assertSameValue(entries[1][0], "p2", "entries[1] key");
  assertSameValue(entries[2][0], "p3", "entries[2] key");
}
