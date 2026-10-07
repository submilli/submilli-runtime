// After a `switch` whose cases match every value and all return, the tested
// value is `never`, as in TypeScript, so the `assertNever` idiom type-checks.
// The code there is unreachable, which is the only error (spec §1.5).
// expect-error: unreachable code
// expect-error: unreachable code
// expect-error-count: 2
type Shape = { kind: "square"; size: number } | { kind: "circle"; radius: number };

function assertNever(value: never): number {
  throw new Error("unexpected value");
}

function area(shape: Shape): number {
  switch (shape.kind) {
    case "square":
      return shape.size * shape.size;
    case "circle":
      return shape.radius * shape.radius;
  }
  return assertNever(shape);
}

function label(value: 1 | 2): string {
  switch (value) {
    case 1:
      return "one";
    case 2:
      return "two";
  }
  const unreachable: never = value;
  return unreachable;
}

function main(): void {
  console.log(area({ kind: "circle", radius: 2 }), label(2));
}
