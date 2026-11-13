// expect-error: narrowing on `v` was dropped by the reassignment
function assertIsString(x: unknown): asserts x is string {
  if (typeof x !== "string") throw new Error("not a string");
}

function main(): void {
  let v: string | number = "abc";
  assertIsString(v);
  v = 4;
  console.log(v.length);
}
