// Class `===` is nominal + structural: same class (vtable identity) and
// field-by-field equal payloads. Method-closure payload slots must not
// participate — two field-equal instances of a class with methods are equal
// even though each holds distinct closure allocations.
class Point {
  x: number;
  y: number;

  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
}

class Unit {}

class Counter {
  n: number;

  constructor(n: number) {
    this.n = n;
  }

  bump(): number {
    return this.n + 1;
  }
}

class Line {
  a: Point;
  b: Point;

  constructor(a: Point, b: Point) {
    this.a = a;
    this.b = b;
  }
}

function main(): void {
  const p = new Point(1, 2);
  assert(p === p);
  assert(new Point(1, 2) === new Point(1, 2));
  assert(new Point(1, 2) !== new Point(1, 3));
  assert(!(new Point(1, 2) === new Point(3, 2)));

  assert(new Unit() === new Unit());

  const c1 = new Counter(5);
  const c2 = new Counter(5);
  assert(c1.bump() === 6);
  assert(c1 === c2);
  assert(new Counter(5) !== new Counter(7));

  const l1 = new Line(new Point(0, 0), new Point(1, 1));
  const l2 = new Line(new Point(0, 0), new Point(1, 1));
  assert(l1 === l2);
  assert(l1 !== new Line(new Point(0, 0), new Point(2, 1)));
}
