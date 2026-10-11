// An array literal led by `null` and `undefined` takes its element type from
// the later values, as in TypeScript.
function count(values: (number | undefined)[]): number {
  return values.length;
}
function first<T>(values: T[]): T | undefined {
  return values.at(0);
}
function pair(): (string | null)[] {
  return [null, "b"];
}
function main(): void {
  assert(count([undefined, 1, 2]) === 3, "a nullish-led argument");
  assert(first([undefined, "x"]) === undefined, "a nullish-led generic argument");
  assert(pair().length === 2, "a nullish-led return value");
  const mixed = [undefined, null, 1];
  const xs = [2, 3];
  const spread = [undefined, ...xs];
  mixed.push(4);
  spread.push(undefined);
  // A leading nullish value gives later elements no context of their own.
  const nested = [null, [1]];
  const objects = [undefined, { a: 2 }];
  const arrows = [undefined, (): number => 3];
  const fromEmpty = [undefined, ...[]];
  const pinned: undefined[] = [...[]];
  assert(nested.length === 2 && objects.length === 2 && arrows.length === 2, "literals after null");
  assert(fromEmpty.length === 1 && pinned.length === 0, "an empty spread takes the running type");
  assert(mixed.length === 4 && mixed.at(2) === 1, "values after null and undefined");
  assert(spread.length === 4 && spread.at(1) === 2, "a spread after undefined");
}
