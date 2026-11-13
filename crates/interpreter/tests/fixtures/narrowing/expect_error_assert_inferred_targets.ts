// tsc narrows after an assertion call only when every name in the call target
// is declared with an explicit type (TS2775), and the target is a name or a
// member of one (TS2776).
// expect-error-count: 5
// expect-error: assertions require every name in the call target to be declared with an explicit type annotation
// expect-error: assertions require the call target to be an identifier or qualified name
function assertIsString(x: unknown): asserts x is string {
  if (typeof x !== "string") throw new Error("not a string");
}

function main(): void {
  const v: unknown = "a";
  const alias = assertIsString;
  alias(v);
  const holder = { check: assertIsString };
  holder.check(v);
  const { check } = holder;
  check(v);
  const checks: (typeof assertIsString)[] = [assertIsString];
  checks[0](v);
  const inner = (x: unknown): asserts x is string => {
    if (typeof x !== "string") throw new Error("not a string");
  };
  inner(v);
}
