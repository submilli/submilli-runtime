class A { a: number = 1; }
class B { b: string = "b"; }
function isAB(x: unknown): x is A | B { return x instanceof A || x instanceof B; }
function textOrNull(x: unknown): x is string | null { return x === null || typeof x === "string"; }
function literals(x: unknown): x is "a" | "b" { return x === "a" || x === "b"; }
function direct(x: unknown): string | null {
  if (!(x !== null && typeof x !== "string")) { return x; }
  return "other";
}
function conditional(x: unknown): string | null {
  return x === null || typeof x === "string" ? x : "other";
}
function main(): void {
  assert(isAB(new A()) && isAB(new B()) && !isAB(5), "class union");
  assert(textOrNull(null) && textOrNull("a") && !textOrNull(5), "null primitive union");
  assert(literals("a") && literals("b") && !literals("c"), "literal union");
  assert(conditional("a") === "a" && conditional(null) === null, "conditional guard");
  assert(direct(null) === null && direct("a") === "a", "negated conjunction");
}
