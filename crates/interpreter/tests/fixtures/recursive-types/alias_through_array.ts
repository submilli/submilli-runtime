// Recursion through an *array* member, the shape a tree takes. It reaches the
// shape collector differently from `Node | null` recursion: a declared type
// still carries its bare `AliasRef` back-edge, while the object literal that
// has to find the matching vtable has had its back-edges rehydrated to the
// inline `Alias` form on the way out of inference. Two spellings of one type
// are two vtable keys, and the literal must not be left looking up the one
// that was never emitted.
type Node = { val: number; kids: Node[] };

// Mutual recursion closes the same cycle through two names.
type Branch = { leaves: Leaf[] };
type Leaf = { branches: Branch[]; weight: number };

// ...and a generic alias carries its argument around the cycle.
type Tree<T> = { value: T; kids: Tree<T>[] };

// Two aliases whose fields share names but not types. They must not collapse
// onto one vtable key: the shape a value carries decides how it serializes.
type SameNamesA = { val: number; kids: SameNamesA[] };
type SameNamesB = { val: string; kids: SameNamesB[] };

function total(n: Node): number {
  let sum: number = n.val;
  for (const kid of n.kids) {
    sum = sum + total(kid);
  }
  return sum;
}

function main(): void {
  const a: Node = { val: 1, kids: [] };
  const b: Node = { val: 2, kids: [a] };
  const c: Node = { val: 4, kids: [b, a] };
  assert(total(c) === 8, "walked a tree built from an array-recursive alias");
  assert(c.kids[0].kids[0].val === 1, "read two levels down");

  const leaf: Leaf = { branches: [], weight: 3 };
  const branch: Branch = { leaves: [leaf] };
  assert(branch.leaves[0].weight === 3, "mutually recursive aliases resolve");

  const tip: Tree<string> = { value: "tip", kids: [] };
  const root: Tree<string> = { value: "root", kids: [tip] };
  assert(root.kids[0].value === "tip", "generic recursive alias resolves");

  const sameA: SameNamesA = { val: 1, kids: [] };
  const sameB: SameNamesB = { val: "s", kids: [] };
  assert(JSON.stringify(sameA) === "{\"kids\":[],\"val\":1}", "A keeps its own shape");
  assert(JSON.stringify(sameB) === "{\"kids\":[],\"val\":\"s\"}", "B keeps its own shape");

  // The contextual type has to survive the back-edge, or a literal that needs a
  // hint — an empty array — has nothing to infer its element type from.
  const nested: Node = { val: 1, kids: [{ val: 2, kids: [{ val: 3, kids: [] }] }] };
  assert(nested.kids[0].kids[0].val === 3, "hint reaches through two back-edges");
  assert(
    JSON.stringify(nested.kids[0].kids[0]) === "{\"kids\":[],\"val\":3}",
    "the deepest literal still carries the alias's shape",
  );
}
