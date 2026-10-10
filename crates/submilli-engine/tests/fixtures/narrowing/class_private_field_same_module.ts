// A `private` field is visible — and narrowable — from elsewhere in its
// declaring module. The narrow source must resolve fields with the same
// visibility rule the access site uses; a structural form that drops private
// members would fail to reconstruct this path and silently lose the narrowing.
class Inner {
  constructor(private s: string | null) {}
}

class Outer {
  constructor(private inner: Inner | null) {}
}

function read(o: Outer): string {
  if (o.inner !== null && o.inner.s !== null) {
    return o.inner.s;
  }
  return "none";
}

function main(): void {
  assert(read(new Outer(new Inner("hi"))) === "hi", "private field narrowed in-module");
  assert(read(new Outer(new Inner(null))) === "none", "inner private field null");
  assert(read(new Outer(null)) === "none", "outer private field null");
}
