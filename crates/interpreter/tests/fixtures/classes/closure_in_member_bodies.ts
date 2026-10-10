// Closures declared inside class member bodies capture that member's params
// and locals: the capture pass treats each member body as its own function
// scope, so the env layout is populated and codegen has locals to read.
class Adder {
  private base: number;
  constructor(base: number) {
    this.base = base;
    // A closure in the constructor body, capturing a ctor param.
    const bump = (x: number): number => x + base;
    this.base = bump(0);
  }

  addTo(n: number): number {
    // Captures a method parameter.
    const add = (x: number): number => x + n;
    return add(this.base);
  }

  scaled(factor: number): number[] {
    // Captures a method-local `const` and a parameter.
    const offset = 10;
    const f = (x: number): number => x * factor + offset;
    return [1, 2].map(f);
  }

  counted(): number {
    // Captures a mutable local — boxed rather than copied.
    let total = 0;
    const acc = (x: number): void => {
      total = total + x;
    };
    acc(3);
    acc(4);
    return total;
  }

  get doubled(): number {
    const twice = (x: number): number => x * 2;
    return twice(this.base);
  }

  set doubled(v: number) {
    const half = (x: number): number => x / 2;
    this.base = half(v);
  }
}

function main(): void {
  const a = new Adder(5);
  assert(a.addTo(2) === 7, "closure capturing a method param");
  const scaled = a.scaled(3);
  assert(scaled[0] === 13 && scaled[1] === 16, "closure capturing param + local");
  assert(a.counted() === 7, "closure mutating a captured local");
  assert(a.doubled === 10, "closure inside a getter body");
  a.doubled = 8;
  assert(a.doubled === 8, "closure inside a setter body");
}
