// A tuple-typed return on a generic FREE function — the expected `[T, T]`
// must fork the array literal into tuple-literal inference rather than
// inferring `T[]`. Generic classes already covered this shape; the
// free-function path had no fixture.
//
// The alias forms are the ones that actually regressed: a tuple reached
// through `type P<T> = [T, T]` is still a tuple hint, and both the literal
// fork and index access have to peel to see it.
type Pair = [number, string];
type P<T> = [T, T];

function aliased(): Pair {
  return [1, "x"];
}

function aliasedGeneric<T>(a: T, b: T): P<T> {
  return [a, b];
}
function pairUp<T>(a: T, b: T): [T, T] {
  return [a, b];
}

function pairViaLocal<T>(a: T, b: T): [T, T] {
  const p: [T, T] = [a, b];
  return p;
}

function zip<A, B>(a: A, b: B): [A, B] {
  return [a, b];
}

function pairsOf<T>(a: T, b: T): [T, T][] {
  return [[a, b]];
}

function firstTwo<T>(xs: T[]): [T, T] {
  return [xs[0], xs[1]];
}

class Box<T> {
  constructor(readonly v: T) {}
}

function main(): void {
  assert(aliased()[0] === 1, "aliased tuple return");
  assert(aliased()[1] === "x", "aliased tuple, second slot");
  assert(aliasedGeneric<number>(3, 4)[1] === 4, "generic aliased tuple return");
  assert(aliasedGeneric("a", "b")[0] === "a", "generic aliased tuple, inferred");

  const explicit = pairUp<number>(1, 2);
  assert(explicit[0] === 1);
  assert(explicit[1] === 2);

  // inferred type args
  const inferred = pairUp("a", "b");
  assert(inferred[0] === "a");
  assert(inferred[1] === "b");

  const viaLocal = pairViaLocal<number>(3, 4);
  assert(viaLocal[0] === 3);

  const mixed = zip<number, string>(5, "x");
  assert(mixed[0] === 5);
  assert(mixed[1] === "x");

  const nested = pairsOf<number>(6, 7);
  assert(nested.length === 1);
  assert(nested[0][1] === 7);

  assert(firstTwo<number>([8, 9, 10])[1] === 9);

  // a class-typed T, so the tuple slots hold reference values
  const boxes = pairUp<Box<number>>(new Box<number>(11), new Box<number>(12));
  assert(boxes[0].v === 11);
  assert(boxes[1].v === 12);
}
