// Where two union-typed values are equal, each holds a value the other's
// type allows, whichever side of `===` or `!==` it is on.
type Wide = "a" | "b" | "c";
type Narrow = "a" | "b";

function sharedLeft(wide: Wide, narrow: Narrow): Narrow | "none" {
  if (wide === narrow) {
    return wide;
  }
  return "none";
}

function sharedRight(wide: Wide, narrow: Narrow): Narrow | "none" {
  if (narrow === wide) {
    return wide;
  }
  return "none";
}

function sharedAfterInequality(wide: Wide, narrow: Narrow): Narrow | "none" {
  if (narrow !== wide) {
    return "none";
  }
  return wide;
}

type Circle = { kind: "circle"; radius: number };
type Square = { kind: "square"; side: number };

// Objects whose `kind` can't match aren't the same object.
function sameCircle(shape: Circle | Square | null, circle: Circle | null): number {
  if (shape === circle) {
    return shape === null ? 0 : shape.radius;
  }
  return -1;
}

function main(): void {
  console.log(sharedLeft("a", "a"), sharedLeft("c", "a"));
  console.log(sharedRight("b", "b"), sharedRight("c", "b"));
  console.log(sharedAfterInequality("a", "a"), sharedAfterInequality("c", "a"));
  const circle: Circle = { kind: "circle", radius: 2 };
  console.log(sameCircle(circle, circle), sameCircle(null, null), sameCircle(null, circle));
}
