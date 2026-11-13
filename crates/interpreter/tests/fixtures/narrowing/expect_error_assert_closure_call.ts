// expect-error: assertions require every name in the call target to be declared with an explicit type annotation
const assertIsString = (x: unknown): asserts x is string => {
  if (typeof x !== "string") throw new Error("not a string");
};

function main(): void {
  const v: unknown = "hi";
  assertIsString(v);
}
