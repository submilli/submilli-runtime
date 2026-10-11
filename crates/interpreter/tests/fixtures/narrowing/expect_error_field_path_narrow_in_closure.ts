// expect-error: cannot read field `s` on `Inner | null`: the receiver can be `null`
// A field path asserts something about a heap value, which a write can falsify
// before the closure runs; the narrowing engine's invalidation cannot see
// across the boundary. Only depth-0 `const` roots cross.
class Inner {
  constructor(public s: string | null) {}
}

class Outer {
  constructor(public inner: Inner | null) {}
}

function main(): void {
  const o = new Outer(new Inner("hi"));
  if (o.inner !== null) {
    const f = (bump: number): number => o.inner.s!.length + bump;
    assert(f(0) === 2);
  }
}
