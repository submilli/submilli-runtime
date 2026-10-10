// An unannotated `const` keeps the literal types its value passes through, as
// TypeScript does: a `?:` with `null` or mixed primitives, `??`, `||` and `&&`.
// A `let` copying such a `const` widens the literals.
function pick(): boolean {
  return "ab".length === 2;
}
function tag(t: "a" | null): number {
  return t === null ? 0 : 1;
}
function mixed(m: "on" | 0): number {
  return m === 0 ? 0 : 2;
}
function either(e: "x" | "y"): string {
  return e;
}
function emptyOr(v: "" | "t"): number {
  return v.length;
}

const moduleTag = pick() ? "a" : null;

function main(): void {
  const cond = pick();
  const optional = cond ? "a" : null;
  const onOrZero = cond ? "on" : 0;
  assert(tag(optional) + tag(moduleTag) + mixed(onOrZero) === 4, "a `?:` keeps both sides");

  const chosen: "x" | null = cond ? null : "x";
  const fallback = chosen ?? "y";
  const orElse = chosen || "y";
  assert(either(fallback) === "y" && either(orElse) === "y", "`??` and `||` keep the default");

  const text: string = cond ? "s" : "";
  const andThen = text && "t";
  assert(emptyOr(andThen) === 1, "`&&` keeps its right side's literal");

  let copy = optional;
  copy = "other";
  let copyMixed = onOrZero;
  copyMixed = 5;
  let copyFallback = fallback;
  copyFallback = "z";
  assert(copy === "other" && copyMixed === 5 && copyFallback === "z", "a `let` copy widens");
}
