// A write through a constant key that names a field is a write to that field,
// as in TypeScript: `pair[key] = 5` after `const key = "a"` ends a guard on
// `pair.a` and narrows it to `number`.
// expect-error: cannot read field `length` on non-object type `number`
// expect-error-count: 1
type Pair = { a: string | number; b: string | number };

function byKey(pair: Pair): number {
  const key = "a";
  if (typeof pair.a === "string") {
    pair[key] = 5;
    return pair.a.length;
  }
  return -1;
}

function main(): void {
  console.log(byKey({ a: "x", b: 1 }));
}
