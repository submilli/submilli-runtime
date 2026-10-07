// expect-error: has no elements, so the callback's element parameter receives `undefined`, not a `number`
// expect-error-count: 1
// The element would arrive as `null`, so `v + i` would give `i` where
// JavaScript gives `NaN`; the program is refused instead.
function main(): void {
  console.log(Array.from({ length: 2 }, (v: number, i) => v + i).join(","));
}
