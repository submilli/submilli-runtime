function choose(flag: boolean): string | number {
  let value;
  if (flag) value = "yes"; else value = 3;
  return value;
}
function maybe(flag: boolean): string | undefined {
  let value;
  if (flag) value = "yes";
  return value;
}
function main(): void {
  const kept = [];
  for (const name of ["a", "b"]) kept.push(name);
  assert(kept.join(",") === "a,b", "loop growth");
  const mixed = [];
  mixed.push(1);
  const first: number = mixed[0];
  mixed.unshift("x");
  const both: (number | string)[] = mixed;
  assert(first === 1 && both[0] === "x", "each read sees earlier writes");
  let assigned = [];
  assigned = [2];
  assert(assigned[0] === 2, "reassignment");
  const indexed = [];
  indexed.push(0);
  indexed[0] = 3;
  const indexedValue: number = indexed[0];
  assert(indexedValue === 3, "index growth");
  let result;
  try { result = "ok"; } catch (e) { return; }
  assert(result.toUpperCase() === "OK", "try assignment");
  assert(choose(true) === "yes" && choose(false) === 3, "branch union");
  assert(maybe(false) === undefined && maybe(true) === "yes", "unassigned path");
  const empty = [];
  assert(empty.length === 0, "never filled");
}
