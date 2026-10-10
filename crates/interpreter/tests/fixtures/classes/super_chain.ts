// Multi-level inheritance: chained `super(...)` constructors and chained
// `super.method()` calls (C -> B -> A).
class A {
  a: number;
  constructor(a: number) {
    this.a = a;
  }
  tag(): string {
    return "A" + this.a.toString();
  }
}

class B extends A {
  b: number;
  constructor(a: number, b: number) {
    super(a);
    this.b = b;
  }
  tag(): string {
    return super.tag() + "B" + this.b.toString();
  }
}

class C extends B {
  c: number;
  constructor(a: number, b: number, c: number) {
    super(a, b);
    this.c = c;
  }
  tag(): string {
    return super.tag() + "C" + this.c.toString();
  }
}

function main(): void {
  const x = new C(1, 2, 3);
  assert(x.a === 1);
  assert(x.b === 2);
  assert(x.c === 3);
  // C.tag -> B.tag -> A.tag, each appending its own segment.
  assert(x.tag() === "A1B2C3");
}
