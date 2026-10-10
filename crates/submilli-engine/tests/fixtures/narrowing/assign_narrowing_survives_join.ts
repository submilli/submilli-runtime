// An assignment inside a branch narrows for the code after the `if`: both
// branch exits agree, so the join is their union. The branch body is a block, so
// the narrowing has to survive both the block boundary and the join — it used to
// be discarded at each.

function implicitElse(): string {
  let s: string | null = null;
  if (s === null) {
    s = "x";
  }
  return s.toUpperCase();
}

function explicitElse(flag: boolean): string {
  let s: string | null = null;
  if (flag) {
    s = "a";
  } else {
    s = "b";
  }
  return s.toUpperCase();
}

function bareBlock(): number {
  let n: number | null = null;
  {
    n = 7;
  }
  return n + 1;
}

// One branch assigns, the other narrows through the guard — the union of
// `string` (assigned) and `string` (guard) is still `string`.
function assignedOneSideGuardedOther(s: string | null): string {
  if (s === null) {
    s = "fallback";
  }
  return s.toUpperCase();
}

// A branch that leaves the path nullable keeps the join nullable.
function stillNullableWhenOneBranchIsNull(flag: boolean): string {
  let s: string | null = "a";
  if (flag) {
    s = null;
  } else {
    s = "b";
  }
  return s === null ? "null" : s;
}

// A block that exits early contributes nothing: the narrowing after the `if`
// comes from the surviving branch alone.
function earlyReturnBranch(s: string | null): string {
  if (s === null) {
    return "none";
  }
  return s.toUpperCase();
}

function main(): void {
  assert(implicitElse() === "X", "implicit else joins the assignment");
  assert(explicitElse(true) === "A", "explicit else, then-branch");
  assert(explicitElse(false) === "B", "explicit else, else-branch");
  assert(bareBlock() === 8, "bare block leaves its assignment narrowing behind");
  assert(assignedOneSideGuardedOther(null) === "FALLBACK", "assigned side");
  assert(assignedOneSideGuardedOther("hi") === "HI", "guarded side");
  assert(stillNullableWhenOneBranchIsNull(true) === "null", "null branch stays nullable");
  assert(stillNullableWhenOneBranchIsNull(false) === "b", "non-null branch");
  assert(earlyReturnBranch("q") === "Q", "early return leaves one branch");
}
