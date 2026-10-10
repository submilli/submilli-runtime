// An object literal with a spread picks its union variant by the tag field, so
// the tag keeps its literal type (`"circle"`, not `string`) and the literal
// fits the variant, as in TypeScript. The tag may come before or after spreads.

interface Circle {
  kind: "circle";
  radius: number;
}

interface Square {
  kind: "square";
  side: number;
}

type Shape = Circle | Square;

function area(s: Shape): number {
  if (s.kind === "circle") {
    return 3 * s.radius * s.radius;
  }
  return s.side * s.side;
}

function main(): void {
  const size = { radius: 2 };
  const before: Shape = { kind: "circle", ...size };
  assert(area(before) === 12, "tag before the spread");

  const side = { side: 3 };
  const after: Shape = { ...side, kind: "square" };
  assert(area(after) === 9, "tag after the spread");

  assert(area({ kind: "square", ...side }) === 9, "as an argument");
}
