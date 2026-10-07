// Spreading a value that may be falsy or null copies nothing when it is, as in
// JavaScript: `...(c && { x: 1 })` adds `x` only when `c` holds, and a null
// source adds no fields. Fields such a spread may not add are optional.
type Point = { x: number; y: number };

function pick(b: boolean): boolean {
  return b;
}

function maybePoint(b: boolean): Point | null {
  return b ? { x: 1, y: 2 } : null;
}

function main(): void {
  const added = { a: 0, ...(pick(true) && { x: 1 }) };
  assert(added.x === 1, "a true condition copies the object's fields");
  const skipped = { a: 0, ...(pick(false) && { x: 1 }) };
  assert((skipped.x ?? "absent") === "absent", "a false condition copies nothing");
  assert(JSON.stringify(skipped) === '{"a":0}', "nothing but the earlier field");

  const empty: string = "";
  const label = { ...(empty && { label: empty }) };
  assert((label.label ?? "absent") === "absent", "an empty string copies nothing");
  const named: string = "n";
  const withName = { ...(named && { name: named }) };
  assert(withName.name === "n", "a non-empty string copies the object");

  const point = { x: 9, y: 9, ...maybePoint(true) };
  assert(point.x === 1 && point.y === 2, "a non-null source overwrites");
  const kept = { x: 9, y: 9, ...maybePoint(false) };
  assert(kept.x === 9 && kept.y === 9, "a null source keeps the earlier fields");

  const flipped = { z: 3, ...(pick(true) ? null : { z: 4 }) };
  assert(flipped.z === 3, "a null branch copies nothing");

  let o = { x: 15, y: 16 };
  o = { ...o, ...(pick(true) && { x: 14 }) };
  assert(o.x === 14 && o.y === 16, "assignable back to the earlier type");
}
