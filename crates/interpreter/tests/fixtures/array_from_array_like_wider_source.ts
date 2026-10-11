// An array-like source with more fields than `length` is still an array-like,
// as an assignment to `{ length: number }` accepts it. The element parameter
// may admit `undefined`, which is what it receives.
function main(): void {
  const source = { length: 2, extra: 1 };
  assert(Array.from(source, (_, i) => i).join(",") === "0,1", "a wider source");
  assert(Array.from(source).length === 2, "a wider source without a callback");
  const optional = Array.from({ length: 2 }, (v: number | undefined, i: number) => v ?? i);
  assert(optional.join(",") === "0,1", "an element type that admits undefined");
  console.log(Array.from(source, (_, i) => i).join(","), optional.join(","));
}
