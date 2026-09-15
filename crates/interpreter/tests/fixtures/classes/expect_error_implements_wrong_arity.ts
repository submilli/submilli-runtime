// expect-error: does not implement
// expect-error: member `scale` has an incompatible signature
// `Circle.scale` takes no parameters, but `Shape.scale` requires one. Arity is
// part of the structural contract, so conformance fails.
interface Shape {
  scale(factor: number): number;
}

class Circle implements Shape {
  scale(): number {
    return 0;
  }
}

function main(): void {}
