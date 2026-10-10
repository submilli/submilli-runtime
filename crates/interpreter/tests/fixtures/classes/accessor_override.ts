// A subclass overrides a getter; a base-typed reference observes the override
// via vtable dispatch.
class Shape {
  get sides(): number {
    return 0;
  }
}

class Triangle extends Shape {
  get sides(): number {
    return 3;
  }
}

function describe(s: Shape): number {
  return s.sides;
}

function main(): void {
  const t = new Triangle();
  assert(t.sides === 3);
  // Base-typed reference dispatches to the override.
  assert(describe(t) === 3);

  const s = new Shape();
  assert(describe(s) === 0);
}
