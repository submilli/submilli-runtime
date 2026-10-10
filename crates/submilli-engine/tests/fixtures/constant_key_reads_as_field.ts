// A literal key, or a constant holding one, that names a declared field reads
// and writes that field: a guard or write through `pair[key]` applies to
// `pair.a`, and the other way round.
type Pair = { a: string | number; b: string | number };
const moduleKey = "b";

function guardByKey(pair: Pair): number {
  const key = "a";
  if (typeof pair[key] === "string") {
    return pair.a.length;
  }
  return -1;
}

function guardByField(pair: Pair): number {
  const key = "a";
  if (typeof pair.a === "string") {
    return pair[key].length;
  }
  return -1;
}

function guardByModuleKey(pair: Pair): number {
  if (typeof pair.b === "string") {
    return pair[moduleKey].length;
  }
  return -1;
}

function writeOtherKey(pair: Pair): number {
  const key = "b";
  if (typeof pair.a === "string") {
    pair[key] = 5;
    return pair.a.length;
  }
  return -1;
}

function writeNarrows(pair: Pair): number {
  const key = "a";
  pair[key] = "xyz";
  pair["b"] = "four";
  return pair.a.length + pair.b.length;
}

function main(): void {
  const pair: Pair = { a: "abc", b: "de" };
  assert(guardByKey(pair) === 3, "a guard through the key narrows the field");
  assert(guardByField(pair) === 3, "a guard on the field narrows the key");
  assert(guardByModuleKey(pair) === 2, "a module constant names the field too");
  assert(writeOtherKey(pair) === 3, "a write to `b` leaves `a` narrowed");
  assert(writeNarrows(pair) === 7, "writes through keys narrow their fields");
  console.log("ok");
}
