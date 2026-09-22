// The other half of the rule: a `const` bound to a bare literal keeps its literal type,
// which is what lets it reach a literal-union parameter. Parentheses group rather than
// compute, so they do not widen.
function f(t: "a" | "b"): number { return t === "a" ? 1 : 2; }
function g(n: 1 | 2): number { return n; }

function main(): void {
  const t = "a";
  const one = (1);
  const alias = one;

  assert(f(t) === 1, "literal reaches a literal union");
  assert(g(one) === 1, "parenthesized literal keeps its type");
  assert(g(alias) === 1, "literal propagates through a const alias");
}
