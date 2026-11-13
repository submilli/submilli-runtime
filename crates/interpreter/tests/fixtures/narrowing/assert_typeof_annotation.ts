// A variable or parameter annotated `typeof f` is declared explicitly, so a
// call through it, or through a member of an annotated object, narrows as a
// call of `f` does.
function assertIsString(x: unknown): asserts x is string {
  if (typeof x !== "string") throw new Error("not a string");
}

const alias: typeof assertIsString = assertIsString;

function length(check: typeof assertIsString, v: unknown): number {
  check(v);
  return v.length;
}

type Checks = { str: typeof assertIsString };

function viaField(checks: Checks, v: unknown): number {
  checks.str(v);
  return v.length;
}

function main(): void {
  const v: unknown = "abcd";
  alias(v);
  assert(v.length === 4, "global alias narrows");
  assert(length(assertIsString, "ab") === 2, "parameter narrows");
  assert(viaField({ str: assertIsString }, "abc") === 3, "member of a parameter narrows");

  const checks: Checks = { str: assertIsString };
  const w: unknown = "a";
  checks.str(w);
  assert(w.length === 1, "member of a local narrows");

  let annotated: typeof assertIsString = assertIsString;
  const x: unknown = "xy";
  annotated(x);
  assert(x.length === 2, "annotated let narrows");

  const y: unknown = "xyz";
  (assertIsString)(y);
  assert(y.length === 3, "parenthesized function narrows");
}
