// The enum members a guard ruled out stay ruled out in a `switch` on the same
// value (its `default` and the code after it) and after a type predicate, so a
// chain of member checks still ends in `never`.
enum S {
  X = "x",
  Y = "y",
  Z = "z",
}

function never(value: never): string {
  return "never";
}

function isS(value: S | number): value is S {
  return typeof value === "string";
}

function afterSwitch(e: S): string {
  switch (e) {
    case S.X:
      return "X";
  }
  if (e === S.Y) return "Y";
  if (e === S.Z) return "Z";
  return never(e);
}

function inDefault(e: S): string {
  switch (e) {
    case S.X:
      return "X";
    default:
      if (e === S.Y) return "Y";
      if (e === S.Z) return "Z";
      return never(e);
  }
}

// A switch whose cases name only the members left is exhaustive.
function switchAfterGuard(e: S): string {
  if (e === S.X) return "X";
  switch (e) {
    case S.Y:
      return "Y";
    case S.Z:
      return "Z";
  }
}

function afterPredicate(e: S | number): string {
  if (e === S.X) return "X";
  if (!isS(e)) return "number";
  if (e === S.Y) return "Y";
  if (e === S.Z) return "Z";
  return never(e);
}

// A module variable a call may change keeps its declared type where every
// member was ruled out, so a stale value read there runs as in JavaScript
// instead of reaching code that trusts `never`.
let mode: S = S.X;

function setMode(value: S): void {
  mode = value;
}

function staleModuleVariable(): string {
  if (mode === S.X) return "X";
  setMode(S.X);
  switch (mode) {
    case S.Y:
      return "Y";
    case S.Z:
      return "Z";
    default:
      const last: S | null = mode;
      return `now ${last}`;
  }
}

function main(): void {
  assert(afterSwitch(S.Z) === "Z");
  assert(inDefault(S.Y) === "Y");
  assert(switchAfterGuard(S.Z) === "Z");
  assert(afterPredicate(3) === "number");
  assert(afterPredicate(S.Z) === "Z");
  mode = S.Y;
  assert(staleModuleVariable() === "now x");
  console.log("ok");
}
