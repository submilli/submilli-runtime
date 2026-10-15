// SUB-1753: `xs.every(guard)` narrows `xs` to the guarded element type, as
// tsc's `this is S[]` overload does.
function isString(x: number | string): x is string {
  return typeof x === "string";
}
function main(): void {
  const xs: (number | string)[] = ["a", "b"];
  if (xs.every(isString)) {
    const ys: string[] = xs;
    assert(ys.join("") === "ab");
  } else {
    assert(false);
  }
  if (xs.every((x): x is string => typeof x === "string")) {
    assert(xs[0].length === 1);
  }
  const mixed: (number | string)[] = [1, "a"];
  assert(!mixed.every(isString));
  const either: string[] | number[] = [1, 2];
  if (!either.every(isString)) {
    const ns: number[] = either;
    assert(ns.length === 2);
  }
}
