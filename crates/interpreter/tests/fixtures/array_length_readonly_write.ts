// SUB-1762: as in tsc, a readonly array's length and a tuple's length can't
// be written.
// expect-error: cannot assign to readonly property `length` on `readonly number[]`
// expect-error: cannot assign to readonly property `length` on `[number, number]`
function main(): void {
  const ro: readonly number[] = [1, 2];
  ro.length = 0;
  const pair: [number, number] = [1, 2];
  pair.length = 0;
}
