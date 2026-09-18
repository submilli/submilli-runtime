class Inner {
  value: number = 2;
}

class Outer {
  inner: Inner | null = new Inner();
}

export function main(): void {
  const outer: Outer | null = new Outer();
  if (outer !== null && outer.inner !== null) {
    assert(outer!.inner.value === 2, "a redundant `!` preserves the field path");
    assert((outer as Outer).inner.value === 2, "a checked cast preserves the field path");
  }
}
