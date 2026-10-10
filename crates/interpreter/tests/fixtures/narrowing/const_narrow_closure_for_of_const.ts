// A per-iteration `for (const v of …)` binding is a `const`, so a closure
// built inside the loop keeps the narrowing for that iteration's value.
class Inner {
  constructor(public n: number) {}
}

function main(): void {
  const xs: Array<Inner | null> = [new Inner(2), null, new Inner(3)];
  let total = 0;
  for (const v of xs) {
    if (v !== null) {
      const f = (bump: number): number => v.n + bump;
      total = total + f(0);
    }
  }
  assert(total === 5, "each iteration's const narrowed independently");
}
