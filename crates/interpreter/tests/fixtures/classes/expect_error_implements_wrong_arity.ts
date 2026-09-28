// expect-error: does not implement
// expect-error: member `scale` has an incompatible signature
// `Circle.scale` requires two parameters, but `Shape.scale` passes one. As in
// TypeScript, a method may declare fewer parameters than the contract, not more.
interface Shape {
  scale(factor: number): number;
}

class Circle implements Shape {
  scale(factor: number, origin: number): number {
    return factor + origin;
  }
}

function main(): void {}
