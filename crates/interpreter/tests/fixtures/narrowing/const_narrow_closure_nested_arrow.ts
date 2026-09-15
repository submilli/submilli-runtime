// An arrow inside an arrow, both reading the narrowed const: the seed is
// re-applied at each closure boundary, so the innermost body still sees it.
class Inner {
  constructor(public n: number) {}
}

function main(): void {
  const i: Inner | null = new Inner(4);
  if (i !== null) {
    const outer = (bump: number): number => {
      const inner = (extra: number): number => i.n + extra;
      return inner(bump);
    };
    assert(outer(1) === 5, "narrowing reaches the inner arrow");
  } else {
    assert(false, "receiver was non-null");
  }
}
