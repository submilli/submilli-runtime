// Regression: a nested narrowing through class-typed fields inside a single
// `&&` chain. The `&&` RHS is inferred under a shadow whose scope ends at the
// condition, so the `if`'s re-run must synthesize its own source from the
// declared types rather than reuse that shadow.
class Inner {
  constructor(public s: string | null) {}
}

class Outer {
  constructor(public inner: Inner | null) {}
}

function via_if(o: Outer): string {
  if (o.inner !== null && o.inner.s !== null) {
    return o.inner.s;
  }
  return "none";
}

function via_ternary(o: Outer): string {
  return o.inner !== null && o.inner.s !== null ? o.inner.s : "none";
}

function main(): void {
  const present = new Outer(new Inner("hi"));
  assert(via_if(present) === "hi", "if-form reads the narrowed field");
  assert(via_ternary(present) === "hi", "ternary form reads the narrowed field");

  assert(via_if(new Outer(new Inner(null))) === "none", "inner field null");
  assert(via_ternary(new Outer(new Inner(null))) === "none", "inner field null, ternary");

  assert(via_if(new Outer(null)) === "none", "outer field null");
  assert(via_ternary(new Outer(null)) === "none", "outer field null, ternary");
}
