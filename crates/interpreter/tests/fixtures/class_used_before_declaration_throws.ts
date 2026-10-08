// A class used before its declaration has run throws a ReferenceError, as
// JavaScript's temporal dead zone does: constructing it, calling a static
// method or testing instanceof. Its own static fields can already use it.
function make(): Box<number> {
  return new Box<number>(1);
}
function zeroX(): number {
  return Point.zero().x;
}
function isPoint(value: unknown): boolean {
  return value instanceof Point;
}

function attempt(run: () => void): string {
  try {
    run();
    return "ran";
  } catch (e) {
    return String(e);
  }
}

const early: string[] = [
  attempt(() => { make(); }),
  attempt(() => { zeroX(); }),
  attempt(() => { isPoint({}); }),
];

class Box<T> {
  constructor(public value: T) {}
}

class Point {
  static readonly unit: Point = new Point(1, 1);
  constructor(public x: number, public y: number) {}
  static zero(): Point {
    return new Point(0, 0);
  }
}

const late: string[] = [
  attempt(() => { make(); }),
  attempt(() => { zeroX(); }),
  attempt(() => { isPoint({}); }),
];

function main(): void {
  assert(early[0] === "ReferenceError: Cannot access 'Box' before initialization", early[0]);
  assert(early[1] === "ReferenceError: Cannot access 'Point' before initialization", early[1]);
  assert(early[2] === "ReferenceError: Cannot access 'Point' before initialization", early[2]);
  assert(late.join(",") === "ran,ran,ran", late.join(","));
  assert(Point.unit.x === 1, "a static field constructs its own class");
}
