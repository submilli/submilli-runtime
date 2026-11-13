// A nested function declaration is declared explicitly, so its assertion
// narrows like a top-level one's.
function main(): void {
  function assertIsString(x: unknown): asserts x is string {
    if (typeof x !== "string") throw new Error("not a string");
  }
  const v: unknown = "abc";
  assertIsString(v);
  assert(v.length === 3, "nested declaration narrows");
}
