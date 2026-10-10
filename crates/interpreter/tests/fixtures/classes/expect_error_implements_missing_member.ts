// expect-error: does not implement
// expect-error: missing member `area`
// expect-error: interface Shape
// A class declaring `implements Shape` must provide every member of `Shape`.
// `Circle` omits `area`, so conformance fails and the diagnostic lifts the
// interface's expected shape.
interface Shape {
  area(): number;
}

class Circle implements Shape {
  radius: number;
  constructor(r: number) {
    this.radius = r;
  }
}

function main(): void {}
