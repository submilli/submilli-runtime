// An array literal whose first element is built from `never` (`[x]` read in
// code no value reaches, or an empty `never[]`) lets a later array element
// fix its element type, and still hints an empty `[]` after it.
function emptyAfterNeverArray(): number {
  const empty: never[] = [];
  const withEmpty = [empty, []];
  const withSpreadEmpty = [empty, ...[]];
  const widened = [empty, [1, 2]];
  return withEmpty.length + withSpreadEmpty.length + widened[1][1];
}

function deadSeed(x: string | number): number {
  if (typeof x !== "string" && typeof x !== "number") {
    const dead = [[x], [1], []];
    return dead.length;
  }
  return 0;
}

function main(): void {
  assert(emptyAfterNeverArray() === 2 + 1 + 2, "a never[] seed gives way to a later array");
  assert(deadSeed("s") === 0, "a dead never seed gives way");
}
