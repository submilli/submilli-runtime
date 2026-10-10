// `===` / `!==` directly on an `unknown`-typed operand (no prior narrowing).
// `unknown` lowers to nullable `(ref null $Object)`, so equality must route
// through the null-aware dispatch: a bare vtable `equals` call expects a
// non-null receiver and produces invalid Wasm (SUB-471).
function main(): void {
  const xs: unknown[] = ["two", 7, true];

  // string element vs a string literal
  assert(xs[0] === "two", "unknown string equals its literal");
  assert(xs[0] !== "other", "unknown string differs from another literal");

  // number element vs a number literal
  assert(xs[1] === 7, "unknown number equals its literal");
  assert(xs[1] !== 8, "unknown number differs from another literal");

  // boolean element vs a boolean literal
  assert(xs[2] === true, "unknown boolean equals its literal");
  assert(xs[2] !== false, "unknown boolean differs from another literal");

  // cross-type comparisons are never equal, never trapping
  assert(xs[0] !== 7, "unknown string differs from a number");
  assert(xs[1] !== "two", "unknown number differs from a string");

  // an unknown holding a null value: equal to null, unequal to a literal
  const n: unknown = null;
  assert(n === null, "null-valued unknown equals null");
  assert(n !== "two", "null-valued unknown differs from a string literal");
  assert(xs[0] !== n, "non-null unknown differs from a null-valued unknown");
}
