// expect-error: cannot read field `length` on `string | null`: the receiver can be `null`
// expect-error: cannot read field `length` on `string | null`: the receiver can be `null`
// An exception can leave a `try` body at any statement, so an assignment inside
// it proves nothing to the `catch` — nor does a `catch`'s own assignment prove
// anything on the path where the body completed. Block-exit propagation stops
// at every `try` clause boundary.
function boom(): string {
  throw new Error("b");
}

function fine(): void {}

function assignedInTry(): number {
  let s: string | null = null;
  try {
    boom();
    s = "x";
  } catch (e) {
    return s.length;
  }
  return 0;
}

function assignedInCatch(): number {
  let s: string | null = null;
  try {
    fine();
  } catch (e) {
    s = "c";
  }
  return s.length;
}

function main(): void {
  assert(assignedInTry() === 0, "unreachable — the program does not compile");
  assert(assignedInCatch() === 0, "unreachable");
}
