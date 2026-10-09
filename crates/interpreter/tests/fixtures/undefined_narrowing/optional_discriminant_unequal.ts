// The unequal side of a test on an optional discriminant keeps the member that
// may omit it, so each branch reads only what every remaining member has.
type S = { kind?: "a"; x: number } | { kind: "b"; y: string };
function describe(s: S): string {
  if (s.kind === undefined) {
    return `missing ${s.x}`;
  }
  if (s.kind !== "b") {
    return `a ${s.x}`;
  }
  return `b ${s.y}`;
}
function main(): void {
  assert(describe({ x: 1 }) === "missing 1", "an omitted kind is the optional member");
  assert(describe({ kind: "a", x: 2 }) === "a 2", "a present kind narrows by value");
  assert(describe({ kind: "b", y: "z" }) === "b z", "the other member keeps its own field");
}
