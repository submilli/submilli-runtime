// Parameter properties: `constructor(public x, private readonly y)` declares and
// assigns fields from the constructor parameters.
class Point {
  constructor(
    public x: number,
    private y: number,
    readonly label: string,
  ) {}

  sum(): number {
    return this.x + this.y;
  }
}

// Mixed with an explicit field and an initializer.
class Mixed {
  scale: number = 2;
  constructor(public base: number) {}

  scaled(): number {
    return this.base * this.scale;
  }
}

class Animal {
  constructor(public name: string) {}
}

// Parameter property passed through to a parent via super.
class Dog extends Animal {
  constructor(
    public breed: string,
    name: string,
  ) {
    super(name);
  }
}

function main(): void {
  const p = new Point(3, 4, "p");
  assert(p.x === 3);
  assert(p.label === "p");
  assert(p.sum() === 7);
  p.x = 10;
  assert(p.x === 10);

  const m = new Mixed(5);
  assert(m.base === 5);
  assert(m.scaled() === 10);

  const d = new Dog("husky", "Rex");
  assert(d.breed === "husky");
  assert(d.name === "Rex");
}
