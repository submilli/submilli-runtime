// An array-like source with more fields than `length` is still an array-like,
// as an assignment to `{ length: number }` accepts it. The element parameter
// may admit `null`, which is what it receives for JavaScript's `undefined`.
function main(): void {
  const source = { length: 2, extra: 1 };
  assert(Array.from(source, (_, i) => i).join(",") === "0,1", "a wider source");
  assert(Array.from(source).length === 2, "a wider source without a callback");
  const nullable = Array.from({ length: 2 }, (v: number | null, i: number) => v ?? i);
  assert(nullable.join(",") === "0,1", "an element type that admits null");
  console.log(Array.from(source, (_, i) => i).join(","), nullable.join(","));
}
