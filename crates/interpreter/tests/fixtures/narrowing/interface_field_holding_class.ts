// An interface property typed as a class instance: the outer hop resolves
// through the interface's structural form, the inner hop through the class
// field map.
class Inner {
  constructor(public s: string | null) {}
}

interface Holder {
  inner: Inner | null;
}

class Impl implements Holder {
  constructor(public inner: Inner | null) {}
}

function read(h: Holder): string {
  if (h.inner !== null && h.inner.s !== null) {
    return h.inner.s;
  }
  return "none";
}

function main(): void {
  assert(read(new Impl(new Inner("hi"))) === "hi");
  assert(read(new Impl(new Inner(null))) === "none");
  assert(read(new Impl(null)) === "none");

  const literal: Holder = { inner: new Inner("obj") };
  assert(read(literal) === "obj", "object-literal receiver, class-typed field");
}
