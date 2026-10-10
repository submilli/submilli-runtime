function main(): void {
  const map = new Map<string, number>();
  const set = new Set<string>();
  map.set("first", 1);
  set.add("first");
  for (let i = 0; i < 100; i++) {
    const key = "temporary" + i.toString();
    map.set(key, i);
    set.add(key);
    assert(map.get(key) === i, "map insertion");
    assert(set.has(key), "set insertion");
    assert(map.delete(key), "map deletion");
    assert(set.delete(key), "set deletion");
    assert(!map.has(key) && !set.has(key), "deleted key absent");
  }
  map.set("second", 2);
  set.add("second");
  map.delete("first");
  set.delete("first");
  map.set("first", 3);
  set.add("first");
  map.set("second", 4);
  set.add("second");
  let mapOrder = "";
  let setOrder = "";
  for (const key of map.keys()) { mapOrder += key + ","; }
  for (const key of set.values()) { setOrder += key + ","; }
  assert(mapOrder === "second,first,", "map reinsertion order");
  assert(setOrder === mapOrder, "set reinsertion order");
  assert(map.size === 2 && set.size === 2, "live counts");
  assert(map.get("second") === 4, "overwrite keeps order");
  const mapCopy = new Map(map);
  const setCopy = new Set(set);
  assert(mapCopy.get("first") === 3, "map copy after reclamation");
  assert(setCopy.has("first"), "set copy after reclamation");
  assert(set.union(new Set(["extra"])).size === 3, "union after reclamation");
  assert(set.intersection(new Set(["first"])).size === 1, "intersection after reclamation");
  map.clear();
  set.clear();
  map.set("fresh", 5);
  set.add("fresh");
  assert(map.get("fresh") === 5 && set.has("fresh"), "reuse after clear");
  assert(!map.delete("missing") && !set.delete("missing"), "missing deletion");
  assert(!map.has("missing"), "missing lookup");
  map.clear();
  set.clear();
  for (let i = 0; i < 100; i++) {
    map.set("only", i);
    set.add("only");
    map.delete("only");
    set.delete("only");
    assert(map.size === 0 && set.size === 0, "empty after deletion");
  }
  existingKeyKeepsIteratorsLive();
}

function existingKeyKeepsIteratorsLive(): void {
  const map = new Map<number, number>();
  const set = new Set<number>();
  for (let i = 0; i < 6; i++) { map.set(i, i); set.add(i); }
  for (let i = 0; i < 4; i++) { map.delete(i); set.delete(i); }
  const values = map.values();
  const elements = set.values();
  map.set(4, 40);
  set.add(4);
  map.delete(5);
  set.delete(5);
  let mapSum = 0;
  let setSum = 0;
  for (const value of values) { mapSum += value; }
  for (const value of elements) { setSum += value; }
  assert(mapSum === 40, "overwrite preserves iterator updates and deletions");
  assert(setSum === 4, "duplicate preserves iterator deletions");
}
