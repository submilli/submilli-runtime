// expect-error: has no elements, so the callback's element parameter receives `undefined`, not a `number`
// expect-error-count: 1
// The element arrives as `undefined`, which a `number` parameter can't
// hold, so the program is refused.
function main(): void {
  console.log(Array.from({ length: 2 }, (v: number, i) => v + i).join(","));
}
