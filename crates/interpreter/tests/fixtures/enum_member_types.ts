// An enum member has its member literal type, as in TypeScript: `E.A` is a
// type, a read of `E.A` has it, and a `let` widens it to `E`. Comparing with a
// member narrows an enum to the members it can still be.

enum Shape {
  Circle = 1,
  Square = 2,
  Triangle = 3,
}

enum Mode {
  Read = "r",
  Write = "w",
}

type Figure = { kind: Shape.Circle; radius: number } | { kind: Shape.Square; side: number };

function area(figure: Figure): number {
  if (figure.kind === Shape.Circle) {
    return 3 * figure.radius * figure.radius;
  }
  return figure.side * figure.side;
}

function shapeName(shape: Shape): string {
  switch (shape) {
    case Shape.Circle:
      return "circle";
    case Shape.Square:
      return "square";
    default: {
      const rest: Shape.Triangle = shape;
      return rest === Shape.Triangle ? "triangle" : "unreachable";
    }
  }
}

function notCircle(shape: Shape): string {
  if (shape !== Shape.Circle) {
    const other: Shape.Square | Shape.Triangle = shape;
    return shapeName(other);
  }
  return "circle";
}

function main(): void {
  const circle: Shape.Circle = Shape.Circle;
  const one: 1 = Shape.Circle;
  const fromNumber: Shape.Square = 2;
  const read: "r" = Mode.Read;
  assert(circle === Shape.Circle && one === 1, "a member read has the member type");
  assert(fromNumber === Shape.Square, "a number literal is the member holding it");
  assert(read === "r", "a string member is its value's literal type");

  let widened = Shape.Circle;
  widened = Shape.Triangle;
  assert(widened === Shape.Triangle, "a `let` widens a member to its enum");

  const both: Shape = Math.random() < 2 ? Shape.Circle : Shape.Square;
  const pair: Shape.Circle | Shape.Square = both === Shape.Circle ? Shape.Circle : Shape.Square;
  assert(pair === Shape.Circle, "members form unions");

  assert(area({ kind: Shape.Circle, radius: 2 }) === 12, "a member tags a union");
  assert(area({ kind: Shape.Square, side: 3 }) === 9, "the other tag narrows too");
  assert(shapeName(Shape.Triangle) === "triangle", "a switch leaves the unnamed members");
  assert(notCircle(Shape.Square) === "square", "`!==` leaves the other members");
  assert(!Shape.Circle === false, "`!` on a member is a literal");

  const unknownValue: unknown = 2;
  let caught = false;
  try {
    console.log(unknownValue as Shape.Circle);
  } catch (err) {
    caught = true;
  }
  assert(caught, "a cast to a member checks the member's value");
  console.log(unknownValue as Shape.Square);
}
