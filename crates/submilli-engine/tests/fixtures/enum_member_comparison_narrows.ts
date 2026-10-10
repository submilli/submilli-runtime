// Comparing with an enum member narrows as a `case` label with it does: an
// `if` chain that rules out every member leaves `never`, `===` on an
// enum-typed discriminant picks its member, and a `default` after a `case` for
// every member drops the enum-typed member. A value still equal to some member
// keeps the enum's type, which has no type for one member.
enum Color { Red, Green, Blue }
enum Side { Left = "left", Right = "right" }
enum Code { A = 1, B = 2 }
type Shape = { tag: Side; size: number } | { tag: "none"; label: string };
type Coded = { tag: Code; n: number } | { tag: 5; text: string };

function assertNever(value: never): number {
  throw new Error("unexpected value");
}

function takesColor(c: Color): number {
  return c;
}

function ifChain(c: Color): number {
  if (c === Color.Red) {
    return 1;
  } else if (c === Color.Green) {
    return 2;
  } else if (c === Color.Blue) {
    return 3;
  }
  return assertNever(c);
}

function keepsEnumType(c: Color): number {
  if (c === Color.Red) {
    return takesColor(c);
  }
  if (c !== Color.Green) {
    return takesColor(c) + 10;
  }
  return takesColor(c) + 20;
}

function stringEnum(side: Side): string {
  if (side === Side.Left) return "L";
  if (side === Side.Right) return "R";
  return String(assertNever(side));
}

function discriminant(shape: Shape): string {
  if (shape.tag === Side.Right) {
    return String(shape.size);
  }
  return "";
}

function defaultAfterEveryMember(coded: Coded): string {
  switch (coded.tag) {
    case Code.A:
      return "a";
    case Code.B:
      return "b";
    default:
      return coded.text;
  }
}

function switchOverEnum(c: Color): number {
  switch (c) {
    case Color.Red:
      return 1;
    case Color.Green:
      return 2;
    case Color.Blue:
      return 3;
    default:
      return assertNever(c);
  }
}

function main(): void {
  assert(ifChain(Color.Blue) === 3, "an if chain over every member");
  assert(keepsEnumType(Color.Red) === 0, "equal to a member, still a Color");
  assert(keepsEnumType(Color.Blue) === 12, "unequal to one member");
  assert(keepsEnumType(Color.Green) === 21, "unequal to two members");
  assert(stringEnum(Side.Right) === "R", "a string enum");
  assert(discriminant({ tag: Side.Right, size: 3 }) === "3", "an enum discriminant");
  assert(defaultAfterEveryMember({ tag: 5, text: "five" }) === "five", "default drops the enum");
  assert(switchOverEnum(Color.Green) === 2, "a switch over every member");
  console.log("ok");
}
