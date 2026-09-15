// expect-error: does not implement
// expect-error: member `area` has an incompatible signature
// expect-error: Circle.area
// `Circle.area` returns `string`, but `Shape.area` is declared to return
// `number`. The member exists but its signature is incompatible; the diagnostic
// lifts the class's actual signature alongside the interface's expected one.
interface Shape {
  area(): number;
}

class Circle implements Shape {
  area(): string {
    return "nope";
  }
}

function main(): void {}
