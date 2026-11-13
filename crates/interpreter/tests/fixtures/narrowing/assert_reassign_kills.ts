// A later assignment ends the narrowing.
function assertIsString(x: unknown): asserts x is string {
  if (typeof x !== "string") throw new Error("not a string");
}

function main(): void {
  let v: string | number = "abc";
  assertIsString(v);
  assert(v.length === 3, "narrowed");
  v = 4;
  assert(v + 1 === 5, "the assignment narrows to number");
}
