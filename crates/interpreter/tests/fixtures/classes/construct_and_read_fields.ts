// Same-package construction (SUB-483): `new Foo(...)` allocates, the constructor
// body assigns fields via `this.x = …`, and public fields read back.
class Point {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
}

class Person {
  name: string;
  private age: number;
  constructor(name: string, age: number) {
    this.name = name;
    this.age = age;
  }
}

function main(): void {
  const p = new Point(3, 4);
  assert(p.x === 3);
  assert(p.y === 4);

  // Field mutation outside the constructor (non-readonly).
  p.x = 10;
  assert(p.x === 10);

  const who = new Person("Ada", 36);
  assert(who.name === "Ada");
}
