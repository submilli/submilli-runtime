// expect-error-count: 3
// expect-error: cannot call `set` on `ReadonlyMap<string, number>`
// expect-error: cannot call `add` on `ReadonlySet<number>`
// expect-error: cannot call `push` on `readonly string[]`
// A readonly collection stays readonly where tsc keeps it so: the collection a
// `ReadonlyMap`/`ReadonlySet` `forEach` callback receives, and a readonly array
// narrowed by an `every` type guard.
function isString(value: unknown): value is string {
  return typeof value === "string";
}
function main(): void {
  const map: ReadonlyMap<string, number> = new Map([["a", 1]]);
  map.forEach((value, key, self) => {
    self.set("b", value);
  });
  const set: ReadonlySet<number> = new Set([1]);
  set.forEach((value, same, self) => {
    self.add(value + 1);
  });
  const values: readonly unknown[] = ["a"];
  if (values.every(isString)) {
    values.push("b");
  }
}
