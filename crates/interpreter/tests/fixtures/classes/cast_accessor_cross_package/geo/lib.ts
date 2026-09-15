class Circle {
  private r: number;
  constructor(r: number) {
    this.r = r;
  }
  get size(): number {
    return this.r * 2;
  }
}

class Square {
  size: number;
  constructor(size: number) {
    this.size = size;
  }
}

class Nameless {
  other: number = 1;
}

/// Accessor-backed, returned opaquely so the consumer never names the class.
export function accessorBacked(r: number): unknown {
  return new Circle(r);
}

/// Data-field-backed, for the contrast case.
export function fieldBacked(size: number): unknown {
  return new Square(size);
}

/// Carries no `size` at all.
export function unrelated(): unknown {
  return new Nameless();
}
