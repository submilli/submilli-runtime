// One member is not another, as in TypeScript (TS2322).
// expect-error: expected `Shape.Square`, got `Shape.Circle`

enum Shape {
  Circle = 1,
  Square = 2,
}

function main(): void {
  const square: Shape.Square = Shape.Circle;
  console.log(square);
}
