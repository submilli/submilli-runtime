// Same-package dynamic-path dispatch (SUB-486): a base class flowing through an
// interface-typed receiver dispatches methods and reads properties structurally —
// the class carries its methods as closures in the object-fields payload, found by
// the same field-name scan object literals use. (Static `c.greet()` still goes
// through the vtable; this exercises the interface-typed path.)
interface Greeter {
  greet(n: number): number;
  name(): string;
  readonly base: number;
  readonly label: string;
}

class Counter implements Greeter {
  base: number;
  label: string;
  constructor(b: number, l: string) {
    this.base = b;
    this.label = l;
  }
  greet(n: number): number {
    return this.base + n;
  }
  name(): string {
    return this.label;
  }
}

function main(): void {
  const g: Greeter = new Counter(10, "ctr");
  assert(g.greet(5) === 15); // dynamic method dispatch, number arg
  assert(g.name() === "ctr"); // dynamic method dispatch, string return
  assert(g.base === 10); // dynamic property read
  assert(g.label === "ctr");

  // Static dispatch on the same instance still works.
  const c = new Counter(1, "x");
  assert(c.greet(2) === 3);
}
