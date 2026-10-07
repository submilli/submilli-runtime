// A call that may run a function assigning a module-level `let` ends the
// variable's narrowing, inside a function as at the top level: the read after
// the call would otherwise see whatever the call left. tsc keeps the narrowing,
// which is unsound here (Node reads `null.length` and throws).
// expect-error: cannot read field `length` on `string | null`: the receiver can be `null`
// expect-error: expected `string`, got `number | string`
// expect-error: cannot read field `length` on `string | null`: the receiver can be `null`
// expect-error: cannot read field `length` on `string | null`: the receiver can be `null`
// expect-error: cannot read field `length` on `string | null`: the receiver can be `null`
// expect-error-count: 5
let label: string | null = "a";
let value: string | number = "s";

function clear(): void {
  label = null;
}

function clearAndAnswer(): boolean {
  label = null;
  return true;
}

function reset(): void {
  value = 5;
}

function guardThenCall(): number {
  if (label !== null) {
    clear();
    return label.length;
  }
  return 0;
}

function assignThenCall(): string {
  value = "abc";
  reset();
  const late: string = value;
  return late;
}

function callInCondition(): number {
  if (label !== null && clearAndAnswer()) {
    return label.length;
  }
  return 0;
}

function callbackMayAssign(): number {
  if (label !== null) {
    [1].forEach((n) => {
      console.log(n);
    });
    return label.length;
  }
  return 0;
}

function callInOneBranch(c: boolean): number {
  if (label !== null) {
    if (c) {
      clear();
    }
    return label.length;
  }
  return 0;
}

function main(): void {
  console.log(guardThenCall(), assignThenCall(), callInCondition(), callbackMayAssign());
  console.log(callInOneBranch(true));
}
