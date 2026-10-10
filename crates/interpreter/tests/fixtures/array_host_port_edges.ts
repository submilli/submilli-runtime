// Edges specific to the Rust host port (SUB-586): an in-place mutator swaps the
// receiver's backing, so a second binding to the same array must observe the
// change; the default `sort` order compares string forms while a comparator
// overrides it; and negative indices resolve from the end.

function main(): void {
  // Mutation is visible through an alias — push/splice/sort recompute the
  // element list and swap the receiver struct's backing, not a private copy.
  const a: number[] = [3, 1, 2];
  const alias = a;
  a.push(4);
  assert(alias.length === 4 && alias[3] === 4, "push is visible through the alias");
  a.splice(0, 1);
  assert(alias.length === 3 && alias[0] === 1, "splice is visible through the alias");
  a.sort();
  assert(alias[0] === 1 && alias[1] === 2 && alias[2] === 4, "sort mutates the shared backing");

  // Default order is by string form: [1, 2, 10] sorts to [1, 10, 2].
  const lex: number[] = [1, 2, 10];
  lex.sort();
  assert(lex[0] === 1 && lex[1] === 10 && lex[2] === 2, "default sort compares string forms");

  // A numeric comparator overrides the default order.
  const num: number[] = [1, 2, 10];
  num.sort((x: number, y: number): number => x - y);
  assert(num[0] === 1 && num[1] === 2 && num[2] === 10, "comparator gives numeric order");

  // Negative indices on at / slice / with resolve from the end.
  const b: number[] = [10, 20, 30, 40];
  assert(b.at(-1) === 40, "at(-1) is the last element");
  assert(b.at(-5) === undefined, "at past the start is undefined");
  const s = b.slice(-2);
  assert(s.length === 2 && s[0] === 30 && s[1] === 40, "slice(-2) is the last two");
  const w = b.with(-1, 99);
  assert(w[3] === 99 && b[3] === 40, "with(-1, v) replaces the last without mutating");

  // Out-of-range `with` throws a catchable Error.
  let threw = false;
  try {
    b.with(10, 0);
  } catch (e) {
    threw = true;
  }
  assert(threw, "with out of range throws");

  // flat flattens to the requested depth; flatMap flattens one level.
  const nested: number[][] = [[1], [2, 3], [4]];
  const flatOne = nested.flat();
  assert(flatOne.length === 4 && flatOne[0] === 1 && flatOne[3] === 4, "flat() concatenates one level");
  const fm = [1, 2, 3].flatMap((x: number): number[] => [x, x * 10]);
  assert(fm.length === 6 && fm[1] === 10 && fm[5] === 30, "flatMap maps then flattens one level");
}
