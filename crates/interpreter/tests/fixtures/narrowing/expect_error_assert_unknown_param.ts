// expect-error: type predicate references unknown parameter `y`
function assertIsString(x: unknown): asserts y is string {
  if (typeof x !== "string") throw new Error("not a string");
}

function main(): void {
  assertIsString("a");
}
