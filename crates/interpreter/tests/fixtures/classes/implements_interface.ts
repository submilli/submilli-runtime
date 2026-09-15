// `implements` (SUB-481) is a typechecker-only contract: a class declaring
// `implements I, J` must structurally satisfy each interface. There are no
// runtime artifacts — the instance flows through interface-typed parameters and
// bindings, dispatching methods through the same `$ObjectShape` getter path an
// object literal uses.
interface Shape {
  area(): number;
  readonly sides: number;
}

interface Named {
  name(): string;
}

class Square implements Shape, Named {
  sides: number;
  private size: number;
  constructor(size: number) {
    this.size = size;
    this.sides = 4;
  }
  area(): number {
    return this.size * this.size;
  }
  name(): string {
    return "square";
  }
}

function totalArea(s: Shape): number {
  return s.area();
}

function main(): void {
  const sq = new Square(3);

  // Class instance flowing through an interface-typed parameter.
  assert(totalArea(sq) === 9);
  assert(sq.sides === 4);

  // Through an interface-typed binding, a different interface it implements.
  const n: Named = sq;
  assert(n.name() === "square");

  // And through an array of the interface type.
  const shapes: Shape[] = [new Square(2), new Square(4)];
  assert(shapes[0].area() === 4);
  assert(shapes[1].area() === 16);
}
