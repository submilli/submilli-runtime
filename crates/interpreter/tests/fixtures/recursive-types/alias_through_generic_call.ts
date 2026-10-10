// A recursive alias flowing through a generic call. Unification peels aliases
// but stops at a back-edge — it is a name, not a body — so the back-edge and
// the alias's own expansion must not be left to compare nominally: `T` bound
// from one argument and re-checked against another would report a bogus
// conflict, and a `T` left bound to the bare back-edge lowers to a slot the
// call site's rehydrated type disagrees with.
type Node = { kids: Node[] };

function firstKid<T>(x: { kids: T[] }): T {
  return x.kids[0];
}

function countBoth<T>(a: { kids: T[] }, b: T): number {
  return a.kids.length;
}

function pick<T>(a: T, b: T): T {
  return a;
}

function main(): void {
  const leaf: Node = { kids: [] };
  const n: Node = { kids: [leaf] };

  // Inferred `T`: bound from the argument's own back-edge, then used as the
  // result type with no annotation to correct it.
  assert(firstKid(n).kids.length === 0, "inferred T from a back-edge");
  const inferred = firstKid(n);
  assert(inferred.kids.length === 0, "inferred T binds a usable slot");

  // The same call with the type argument spelled out, and with a return hint.
  assert(firstKid<Node>(n).kids.length === 0, "explicit type argument");
  const hinted: Node = firstKid(n);
  assert(hinted.kids.length === 0, "return hint unifies against the alias");

  // Two arguments binding one `T` from both spellings — the back-edge inside
  // the first argument's field, the alias body itself in the second.
  assert(countBoth(n, n) === 1, "one T bound from both spellings");
  assert(pick(n, leaf).kids.length === 1, "both arguments spell T the same");
}
