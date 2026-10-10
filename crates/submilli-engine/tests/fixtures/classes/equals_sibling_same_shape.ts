// Nominal `===`: same-shape sibling classes canonicalize to the same WasmGC
// type, so the equals guard must compare vtable identity, not shape — a
// Circle never equals a Square even with identical payloads.
class Shape {
  area: number;

  constructor(area: number) {
    this.area = area;
  }
}

class Circle extends Shape {
  r: number;

  constructor(r: number) {
    super(r * r * 3);
    this.r = r;
  }
}

class Square extends Shape {
  r: number;

  constructor(r: number) {
    super(r * r * 3);
    this.r = r;
  }
}

function main(): void {
  const c: Shape = new Circle(2);
  const s: Shape = new Square(2);
  assert(c !== s);
  assert(s !== c);
  assert(!(c === s));

  const c2: Shape = new Circle(2);
  assert(c === c2);
  assert(new Square(2) === new Square(2));
}
