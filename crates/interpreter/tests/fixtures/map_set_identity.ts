// `Map`/`Set` carry the object vtable but not the `$ObjectShape` payload the
// universal slots read, so every slot needs its own answer. `equals` is
// reference identity (JS semantics), `hash` pairs with it, and `toJson`
// refuses with a message naming the conversion.
function main(): void {
  const m1 = new Map<string, number>();
  const m2 = new Map<string, number>();
  m1.set("k", 1);
  m2.set("k", 1);

  assert(m1 === m1, "a map equals itself");
  assert((m1 === m2) === false, "two maps with equal entries are still distinct");

  const s1 = new Set<number>();
  const s2 = new Set<number>();
  s1.add(7);
  s2.add(7);
  assert(s1 === s1, "a set equals itself");
  assert((s1 === s2) === false, "two sets with equal elements are still distinct");

  // Reference equality and the hash have to agree, or a collection cannot be
  // used as a key of another collection.
  const outer = new Map<Map<string, number>, string>();
  outer.set(m1, "first");
  outer.set(m2, "second");
  assert(outer.size === 2, "distinct maps occupy distinct entries");
  assert(outer.get(m1) === "first", "lookup finds the right map");
  assert(outer.get(m2) === "second", "and the other one");

  const setKeys = new Set<Set<number>>();
  setKeys.add(s1);
  setKeys.add(s2);
  assert(setKeys.size === 2, "distinct sets occupy distinct entries");
  assert(setKeys.has(s1), "a set is findable as a key");

  // Nested in a plain object or an array, the walk reaches the collection
  // through `equals` rather than directly.
  const holderA = { inner: m1 };
  const holderB = { inner: m2 };
  assert((holderA === holderB) === false, "a map nested in an object");

  const listA: unknown[] = [m1];
  const listB: unknown[] = [m2];
  assert((listA === listB) === false, "a map nested in an array");

  const same: unknown[] = [m1];
  const sameAgain: unknown[] = [m1];
  assert(same === sameAgain, "the *same* map nested in two arrays compares equal");
}
