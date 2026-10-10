// A tuple literal checked against an *aliased* tuple annotation. The alias has
// to be peeled before the literal is checked, or the elements get validated
// under array rules — every element against the first element's type — and a
// heterogeneous tuple is rejected as `expected number, got string`.
type Pair = [number, string];
type Chained = Pair;
type Nums = number[];

function makePair(): Pair {
  return [1, "a"];
}

function makeChained(): Chained {
  return [2, "b"];
}

function takesPair(p: Pair): string {
  return p[1];
}

function main(): void {
  const p = makePair();
  assert(p[0] === 1 && p[1] === "a", "tuple literal against an alias return");

  const c = makeChained();
  assert(c[0] === 2 && c[1] === "b", "tuple literal against a chained alias return");

  assert(takesPair([3, "c"]) === "c", "tuple literal against an aliased parameter");

  const direct: Pair = [4, "d"];
  assert(direct[1] === "d", "tuple literal against an aliased local annotation");

  const arr: Nums = [1, 2, 3];
  assert(arr[1] === 2, "array literal against an aliased local annotation");
}
