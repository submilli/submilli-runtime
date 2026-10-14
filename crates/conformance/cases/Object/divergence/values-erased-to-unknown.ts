// Not a test262 port: pins a documented divergence in the enumeration
// statics. Object.values / entries erase element types to `unknown` —
// heterogeneous fields need narrowing to be used.

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
}
