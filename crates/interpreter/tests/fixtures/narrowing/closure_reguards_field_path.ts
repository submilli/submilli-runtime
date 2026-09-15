// The prescribed workaround for the shapes that do not cross a closure
// boundary: hoist the field path into a `const` first, then narrow that.
class Inner {
  constructor(public s: string | null) {}
}

class Outer {
  constructor(public inner: Inner | null) {}
}

function main(): void {
  const o = new Outer(new Inner("hoisted"));
  const inner = o.inner;
  if (inner !== null && inner.s !== null) {
    const s = inner.s;
    const f = (bump: number): number => s.length + bump;
    assert(f(0) === 7, "hoisted const carries the narrowing into the closure");
  } else {
    assert(false, "both hops were non-null");
  }
}
