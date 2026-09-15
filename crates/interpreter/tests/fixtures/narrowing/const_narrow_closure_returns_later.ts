// The closure escapes the narrowing region and is invoked afterwards. The
// re-materialized cast is evaluated per call, so it must still hold.
class Inner {
  constructor(public n: number) {}
}

function call_it(f: (bump: number) => number): number {
  return f(10);
}

function main(): void {
  const i: Inner | null = new Inner(7);
  if (i !== null) {
    const g = (bump: number): number => i.n + bump;
    assert(call_it(g) === 17, "cast re-checked at the later call site");
  } else {
    assert(false, "receiver was non-null");
  }
}
