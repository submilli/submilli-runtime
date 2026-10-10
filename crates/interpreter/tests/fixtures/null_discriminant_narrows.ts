// A discriminant that is `null` in one member narrows through `=== null`,
// `!== null` and `case null`, as a literal one does, and a `switch` whose
// cases cover `null` and every literal is exhaustive.
type Tagged = { kind: "a"; x: number } | { kind: null; u: number };
type Mixed = { kind: "a" | null; x: number } | { kind: "b"; u: number };

function byEquality(v: Tagged): number {
  if (v.kind === null) return v.u;
  return v.x;
}

function byInequality(v: Tagged): number {
  if (v.kind !== null) return v.x;
  return v.u;
}

function bySwitch(v: Tagged): number {
  switch (v.kind) {
    case "a":
      return v.x;
    case null:
      return v.u;
  }
}

function partly(m: Mixed): number {
  if (m.kind === null) return m.x;
  if (m.kind === "b") return m.u;
  return m.x;
}

function main(): void {
  assert(byEquality({ kind: null, u: 1 }) === 1, "=== null picks the null member");
  assert(byEquality({ kind: "a", x: 2 }) === 2, "the rest is the literal member");
  assert(byInequality({ kind: "a", x: 3 }) === 3, "!== null picks the literal member");
  assert(bySwitch({ kind: null, u: 4 }) === 4, "case null picks the null member");
  assert(partly({ kind: null, x: 5 }) === 5, "a member that may be null stays");
  assert(partly({ kind: "b", u: 6 }) === 6, "and leaves only where it can't be");
  console.log("ok");
}
