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

function main(): void {
  console.log(sharedLeft("a", "a"), sharedLeft("c", "a"));
  console.log(sharedRight("b", "b"), sharedRight("c", "b"));
  console.log(sharedAfterInequality("a", "a"), sharedAfterInequality("c", "a"));
}
