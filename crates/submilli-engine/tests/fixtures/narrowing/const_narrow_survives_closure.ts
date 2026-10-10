// TypeScript parity: a depth-0 narrowing on a `const` survives into a closure
// body. The `const` can never be reassigned, so re-reading it inside the
// closure yields the value the guard tested. The region is re-emitted inside
// the body over a fresh read — the shadow local never crosses the frame.
class Inner {
  constructor(public n: number) {}
}

function main(): void {
  const i: Inner | null = new Inner(3);
  if (i !== null) {
    const f = (bump: number): number => i.n + bump;
    assert(f(0) === 3, "class-typed const narrowed inside the closure");
    assert(f(10) === 13, "closure still takes its own args");
  } else {
    assert(false, "receiver was non-null");
  }

  const s: string | null = "hello";
  if (s !== null) {
    const len = (bump: number): number => s.length + bump;
    assert(len(0) === 5, "string const narrowed inside the closure");
  }
}
