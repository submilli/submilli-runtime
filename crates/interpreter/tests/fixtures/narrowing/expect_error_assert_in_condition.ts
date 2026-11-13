// expect-error: expected a value in this condition, got `void`
function assertIsString(x: unknown): asserts x is string {
  if (typeof x !== "string") throw new Error("not a string");
}

function main(): void {
  const v: unknown = "hi";
  if (assertIsString(v)) {
    console.log("never");
  }
}
