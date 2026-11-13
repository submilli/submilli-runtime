// An assertion in one branch narrows only that branch, and one in a loop body
// does not narrow the loop's next iteration.
// expect-error-count: 2
// expect-error: cannot read field `length` on `unknown`
function assertIsString(x: unknown): asserts x is string {
  if (typeof x !== "string") throw new Error("not a string");
}

function afterBranch(v: unknown, c: boolean): number {
  if (c) assertIsString(v);
  return v.length;
}

function inLoop(v: unknown): number {
  let total = 0;
  for (let i = 0; i < 2; i++) {
    total += v.length;
    assertIsString(v);
  }
  return total;
}

function main(): void {
  afterBranch("a", true);
  inLoop("a");
}
