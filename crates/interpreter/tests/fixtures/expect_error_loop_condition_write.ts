// A loop condition is checked where the state before the loop meets every
// back edge, so a read it makes before its own write sees what the body left.
// Each line below is an error `tsc --strict` reports too.
// expect-error-count: 11
// expect-error: cannot read field `toFixed` on non-object type `number | string`
// expect-error: cannot read field `length` on non-object type `5 | string`
// expect-error: expected `number`, got `string`
// expect-error: cannot read field `length` on non-object type `number | string`
// expect-error: expected `string`, got `number | string`

function flag(): boolean {
  return false;
}

function noReset(): void {
  let x: string | number = 5;
  x = 5;
  while ((x = x.toFixed()) !== "") {}
}

function readBeforeWrite(): void {
  let x: string | number = "a";
  x = "a";
  while (x.length > 0 && (x = 5) === 5) {}
}

function continueSkipsReset(): void {
  let x: string | number = 5;
  x = 5;
  while ((x = x.toFixed()) !== "") {
    if (flag()) continue;
    x = 2;
  }
}

function updateWritesTheWrongType(): void {
  let x: string | number = 5;
  x = 5;
  for (; (x = x.toFixed()) !== ""; x = "s") {}
}

function bodyReadsTheConditionsWrite(): void {
  let x: string | number = 5;
  x = 5;
  while ((x = x.toFixed()) !== "") {
    const n: number = x;
    x = 5;
  }
}

function mixed(): string | number {
  return 5;
}

// With no back edge, the condition's own write still comes after its read.
function readBeforeWriteWithoutBackEdge(): void {
  let x: string | number = mixed();
  while (x.length + (x = "s").length > 0) {
    break;
  }
}

// The condition's true branch narrows `y` to what `x` is, which the body changes.
function conditionDependsOnTheBody(): void {
  let x: string | number = "a";
  x = "a";
  let y: string | number = "b";
  while ((y = x) !== "zz") {
    const s: string = y;
    x = 5;
  }
}

function conditionWriteDependsOnTheBody(): void {
  let x: string | number = "a";
  x = "a";
  while ((x = [x][0]) !== "zz") {
    const s: string = x;
    x = 5;
  }
}

// A guard holds again on each pass, but narrows what the body left.
function guardSeesWhatTheBodyLeft(): void {
  let x: string | number | null = "a";
  x = "a";
  while (x !== null) {
    const s: string = x;
    x = 5;
  }
}

// Only one side of `||` needs to hold on the next pass.
function eitherSideHolds(): void {
  let x: string | number | null = "a";
  x = "a";
  let y: string | null = "q";
  y = "q";
  let i = 0;
  while (y === null || i < 3) {
    const s: string = x;
    i++;
    x = 5;
    y = null;
  }
}

function isWord(u: string | number): u is string {
  return typeof u === "string" && u.length > 0;
}

// A type guard that returns false proves nothing, so `!isWord(v)` can hold again.
function negatedGuardCanHoldAgain(): void {
  let x: string | number | null = "a";
  x = "a";
  let v: string | number = 1;
  v = 1;
  while (!isWord(v)) {
    const s: string = x;
    x = 5;
    v = "";
  }
}

function main(): void {
  noReset();
  negatedGuardCanHoldAgain();
  eitherSideHolds();
  guardSeesWhatTheBodyLeft();
  readBeforeWriteWithoutBackEdge();
  conditionDependsOnTheBody();
  conditionWriteDependsOnTheBody();
  readBeforeWrite();
  continueSkipsReset();
  updateWritesTheWrongType();
  bodyReadsTheConditionsWrite();
}
