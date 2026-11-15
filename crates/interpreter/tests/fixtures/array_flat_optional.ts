function optionalDefault(xs: number[][] | undefined): number[] | undefined {
  return xs?.flat();
}
function optionalDepth(xs: number[][][] | undefined, depth: number): (number | number[] | number[][])[] | undefined {
  return xs?.flat(depth);
}
function optionalCall(xs: number[][][]): number[] | undefined {
  return xs.flat?.(2);
}
interface Counter { count: number }
function depth(counter: Counter): number { counter.count++; return 1; }
function skipped(xs: number[][] | undefined, counter: Counter): (number | number[])[] | undefined {
  return xs?.flat(depth(counter));
}
function main(): void {
  assert(JSON.stringify(optionalDefault([[1], [2]])) === "[1,2]", "optional receiver default");
  assert(optionalDefault(undefined) === undefined, "optional receiver absent");
  assert(JSON.stringify(optionalDepth([[[3]]], 2)) === "[3]", "optional receiver dynamic depth");
  assert(JSON.stringify(optionalCall([[[4]]])) === "[4]", "optional method call");
  const counter: Counter = { count: 0 };
  assert(skipped(undefined, counter) === undefined, "absent receiver skips call");
  assert(counter.count === 0, "depth side effect skipped");
  assert(JSON.stringify(skipped([[6]], counter)) === "[6]", "present receiver calls");
  assert(counter.count === 1, "depth evaluated once");
}
