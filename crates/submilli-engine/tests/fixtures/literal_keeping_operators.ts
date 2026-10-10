// Conditional and logical expressions keep their operands' literal types, as
// in TypeScript, and a negated number literal is a literal. A `let`, an array
// element or an object property bound to one widens.

type Sign = -1 | 0 | 1;
type Warmth = "warm" | "cool";

function sign(x: number): Sign {
  return x < 0 ? -1 : x === 0 ? 0 : 1;
}

function warmth(celsius: number): string {
  const label = celsius > 25 ? "warm" : "cool";
  const kept: Warmth = label;
  return kept;
}

function main(): void {
  const minusOne = -1;
  const exact: -1 = minusOne;
  assert(exact === -1 && sign(-5) === -1 && sign(0) === 0, "`-1` is a literal");

  assert(warmth(30) === "warm" && warmth(10) === "cool", "a ternary keeps its literals");

  let widened = Math.random() < 2 ? "warm" : "cool";
  widened = "lukewarm";
  assert(widened === "lukewarm", "a `let` bound to a ternary widens");

  const values = [Math.random() < 2 ? 1 : 2];
  values.push(5);
  assert(values.length === 2, "an array element widens");

  const holder = { mode: Math.random() < 2 ? "on" : "off" };
  holder.mode = "auto";
  assert(holder.mode === "auto", "an object property widens");

  const name = "foo";
  const first = name || "bar";
  const onlyFoo: "foo" = first;
  assert(onlyFoo === "foo", "`a || b` is `a` when `a` is never falsy");

  const maybe: string = Math.random() < 2 ? "" : "x";
  const fallback = maybe || "default";
  assert(fallback === "default", "`a || b` still includes `b` when `a` can be falsy");

  const nothing: string | null = null;
  const chosen: "picked" | string = nothing ?? "picked";
  assert(chosen === "picked", "`??` keeps its operands' literals");
}
