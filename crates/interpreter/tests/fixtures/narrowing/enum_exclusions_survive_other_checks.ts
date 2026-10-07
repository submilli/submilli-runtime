// An enum member a guard ruled out stays ruled out through a later `null`,
// `typeof` or truthiness check on the same variable, so the chain of member
// checks still ends in `never`.
enum S {
  X = "x",
  Y = "y",
}

function never(value: never): string {
  return "never";
}

function afterNull(s: S | null): string {
  if (s === S.X) return "X";
  if (s === null) return "null";
  if (s === S.Y) return "Y";
  return never(s);
}

function afterTypeof(s: S | number): string {
  if (s === S.X) return "X";
  if (typeof s === "number") return "number";
  if (s === S.Y) return "Y";
  return never(s);
}

function afterTruthiness(s: S | null): string {
  if (s === S.X) return "X";
  if (!s) return "null";
  if (s === S.Y) return "Y";
  return never(s);
}

function main(): void {
  assert(afterNull(S.Y) === "Y");
  assert(afterNull(null) === "null");
  assert(afterTypeof(S.Y) === "Y");
  assert(afterTypeof(3) === "number");
  assert(afterTruthiness(S.Y) === "Y");
  assert(afterTruthiness(null) === "null");
}
