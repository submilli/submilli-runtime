// SUB-2148: an index read is typed `T`, as in tsc's `--strict`, and a missing
// element or key is decided where the read is checked. The idioms that check
// for a missing value run as in Node, with no warning.
// expect-error-count: 0
interface Item {
  count: number;
}

function main(): void {
  const array: string[] = ["1", "2", "3"];
  const x = array[100] ?? "missing";
  assert(x === "missing", "a missing element takes the default");

  const counts: Record<string, number> = {};
  for (const k of ["a", "b", "a"]) {
    counts[k] = (counts[k] ?? 0) + 1;
  }
  assert(counts["a"] === 2 && counts["b"] === 1, "counting");

  const groups: Record<string, string[]> = {};
  for (const k of ["x", "y", "x"]) {
    if (!groups[k]) groups[k] = [];
    groups[k].push(k);
  }
  assert(groups["x"].length === 2 && groups["y"].length === 1, "grouping");

  const byId: Record<string, Item> = {};
  for (const id of ["u1", "u2", "u1"]) {
    const existing = byId[id];
    if (!existing) byId[id] = { count: 1 };
    else existing.count += 1;
  }
  assert(byId["u1"].count === 2 && byId["u2"].count === 1, "stored, then checked");
}
