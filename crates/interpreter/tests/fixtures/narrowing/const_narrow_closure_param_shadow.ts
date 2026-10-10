// The closure declares its own parameter with the same name as a narrowed
// outer `const`. The param is a different binding, so it must NOT inherit the
// narrowing — seeding it would emit a read of the param and cast a value the
// guard never tested.
function main(): void {
  const x: string | null = "outer";
  if (x !== null) {
    const f = (x: string | null): string => (x === null ? "inner-null" : "inner-" + x);
    assert(f(null) === "inner-null", "shadowing param stays nullable");
    assert(f("v") === "inner-v", "shadowing param narrows on its own");
    assert(x === "outer", "outer const is untouched");
  } else {
    assert(false, "x is non-null");
  }
}
