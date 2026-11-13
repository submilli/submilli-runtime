// An assertion narrows a property path, as a guard does.
function assertIsString(x: unknown): asserts x is string {
  if (typeof x !== "string") throw new Error("not a string");
}

type Box = { inner: { value: string | number } };

function main(): void {
  const box: Box = { inner: { value: "abc" } };
  assertIsString(box.inner.value);
  assert(box.inner.value.length === 3, "field path narrowed");
  {
    assertIsString(box.inner.value);
    assert(box.inner.value.toUpperCase() === "ABC", "narrowed in a nested block");
  }
}
