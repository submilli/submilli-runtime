// A loop condition is checked where the state before the loop meets every
// back edge, so a read it makes before its own write sees what the body left.
// Each line below is an error `tsc --strict` reports too.
// expect-error-count: 5
// expect-error: cannot read field `toFixed` on non-object type `number | string`
// expect-error: cannot read field `length` on non-object type `5 | string`
// expect-error: expected `number`, got `string`

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

function main(): void {
  noReset();
  readBeforeWrite();
  continueSkipsReset();
  updateWritesTheWrongType();
  bodyReadsTheConditionsWrite();
}
