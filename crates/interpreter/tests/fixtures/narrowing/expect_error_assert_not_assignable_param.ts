// expect-error: a type predicate's type must be assignable to its parameter's type
function assertIsBoolean(x: string | number): asserts x is boolean {
  throw new Error("never");
}

function main(): void {
  assertIsBoolean(1);
}
