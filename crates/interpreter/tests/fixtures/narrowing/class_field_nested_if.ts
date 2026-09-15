// The two-nested-`if` spelling of `class_field_and_chain` (which already
// worked), plus the early-return implicit-else join on a class field path —
// that join reuses the stored view source, so it exercises a second install
// path for the same shadow.
class Inner {
  constructor(public s: string | null) {}
}

class Outer {
  constructor(public inner: Inner | null) {}
}

function nested(o: Outer): string {
  if (o.inner !== null) {
    if (o.inner.s !== null) {
      return o.inner.s;
    }
  }
  return "none";
}

function early_return(o: Outer): string {
  if (o.inner === null) {
    return "none";
  }
  // `o.inner` is narrowed by the implicit else, past the `if`
  if (o.inner.s === null) {
    return "empty";
  }
  return o.inner.s;
}

function main(): void {
  assert(nested(new Outer(new Inner("hi"))) === "hi");
  assert(nested(new Outer(new Inner(null))) === "none");
  assert(nested(new Outer(null)) === "none");

  assert(early_return(new Outer(new Inner("hi"))) === "hi");
  assert(early_return(new Outer(new Inner(null))) === "empty");
  assert(early_return(new Outer(null)) === "none");
}
