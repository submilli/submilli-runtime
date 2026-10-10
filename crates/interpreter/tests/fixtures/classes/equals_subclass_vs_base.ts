// Exact-class semantics: a subclass instance never equals a base instance, in
// either dispatch direction. Two base-typed variables holding subclass twins
// are equal — dispatch runs off the runtime vtable, and the subclass body
// compares the flattened field list including inherited fields.
class Point {
  x: number;
  y: number;

  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
}

class Point3D extends Point {
  z: number;

  constructor(x: number, y: number, z: number) {
    super(x, y);
    this.z = z;
  }
}

function main(): void {
  const base: Point = new Point(1, 2);
  const sub: Point = new Point3D(1, 2, 3);
  assert(base !== sub);
  assert(sub !== base);

  const sub2: Point = new Point3D(1, 2, 3);
  assert(sub === sub2);
  assert(sub !== new Point3D(1, 2, 4));
}
