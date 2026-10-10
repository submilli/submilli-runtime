// `typeof x === "function"` narrows a union to its function members, and the
// else branch to the rest.
function callOrLength(x: string | (() => number)): number {
  if (typeof x === "function") {
    return x();
  }
  return x.length;
}

function callOrDefault(x: (() => number) | null): number {
  if (typeof x === "function") {
    return x();
  }
  return -1;
}

function doubled(x: number | ((a: number) => number) | { k: number }): number {
  if (typeof x !== "function") {
    return 0;
  }
  return x(2);
}

function main(): void {
  assert(callOrLength(() => 3) === 3);
  assert(callOrLength("ab") === 2);
  assert(callOrDefault(null) === -1);
  assert(callOrDefault(() => 5) === 5);
  assert(doubled((a) => a * 2) === 4);
  assert(doubled(4) === 0);
  assert(doubled({ k: 1 }) === 0);
}
