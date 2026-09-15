// A `typeof` narrowing on an `unknown` const. The seeded region computes its
// cast from (declared, narrowed) directly rather than replaying the outer
// chain, so a narrowing that reached `string` via `unknown` must still cast.
function main(): void {
  const u: unknown = "hi";
  if (typeof u === "string") {
    const f = (bump: number): number => u.length + bump;
    assert(f(0) === 2, "unknown narrowed to string inside the closure");
  } else {
    assert(false, "u is a string");
  }
}
