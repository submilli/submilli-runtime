// Destructuring patterns may declare the parameters of class instance and
// static methods, constructors (before or after `super`), and setters.
class Base {
  n: number;
  constructor(n: number) {
    this.n = n;
  }
}

class Point extends Base {
  extra: number = 5;

  constructor({ a, b }: { a: number; b: number }, [c]: number[]) {
    super(a);
    this.extra += b + c;
  }

  sum({ x }: { x: number }): number {
    return x + this.n;
  }

  static product([x, y]: number[]): number {
    return x * y;
  }

  static first<T>([head]: T[]): T {
    return head;
  }

  set reset({ n }: { n: number }) {
    this.n = n;
  }
}

class Box<T> {
  v: T;
  n: number;
  constructor({ v, n }: { v: T; n: number }) {
    this.v = v;
    this.n = n;
  }
}

class GenericChild<T> extends Box<T> {
  constructor({ v }: { v: T }, [n]: number[]) {
    super({ v, n });
  }
}

function main(): void {
  const point = new Point({ a: 1, b: 2 }, [3]);
  assert(point.n === 1 && point.extra === 10, "constructor patterns");
  assert(point.sum({ x: 4 }) === 5, "an instance method pattern");
  assert(Point.product([3, 4]) === 12, "a static method pattern");
  assert(Point.first(["x", "y"]) === "x", "a generic static method pattern");
  point.reset = { n: 9 };
  assert(point.n === 9, "a setter pattern");
  const child = new GenericChild<string>({ v: "gv" }, [7]);
  assert(child.v === "gv" && child.n === 7, "a generic subclass passes destructured values to super");
}
