function main(): void {
  const a = new Set<number | null>([null, 0, 1]);
  const b = new Set<number | null>([null, 0, 2]);
  assert(JSON.stringify(Array.from(a.union(b))) === '[null,0,1,2]');
  assert(JSON.stringify(Array.from(a.intersection(b))) === '[null,0]');
  assert(JSON.stringify(Array.from(a.difference(b))) === '[1]');
  assert(JSON.stringify(Array.from(a.symmetricDifference(b))) === '[1,2]');
  assert(new Set<number | null>([null]).isSubsetOf(a));
  assert(a.isSupersetOf(new Set<number | null>([null])));
  assert(!a.isDisjointFrom(new Set<number | null>([null])));
  const m = new Map<unknown, number>();
  const key = {};
  m.set(null, 1); m.set(0, 2); m.set(key, 3); m.set(false, 4);
  assert(m.get(null) === 1 && m.get(0) === 2 && m.get(key) === 3 && m.get(false) === 4);
}
