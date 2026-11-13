// expect-error: asserts x is string
function assertIsString(x: unknown): asserts x is string {
  if (typeof x !== "string") throw new Error("not a string");
}

function main(): void {
  assertIsString();
}
