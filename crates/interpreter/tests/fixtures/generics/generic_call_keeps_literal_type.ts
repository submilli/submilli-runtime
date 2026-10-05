// A literal passed straight for a type parameter that is the call's result
// keeps its literal type, as in tsc: `id(1)` is `1`. So does the return of a
// function literal passed for a bare type parameter: `id(() => 42)` is
// `() => 42` when it is the type parameter's only candidate. A `let` still
// widens what it copies.
function id<T>(x: T): T {
  return x;
}

function maybe<T>(x: T, keep: boolean): T | null {
  return keep ? x : null;
}

function pick<T>(first: T, second: T): T {
  return second;
}

function main(): void {
  const one: 1 = id(1);
  const word: "s" = id("s");
  const yes: true = id(true);
  const kept: "k" | null = maybe("k", true);
  assert(one === 1 && word === "s" && yes && kept === "k", "the literal types");

  const answer: () => 42 = id(() => 42);
  const branches: () => number = id(() => {
    if (one > 5) {
      return 1;
    }
    return 2;
  });
  assert(answer() === 42 && branches() === 2, "returned literals");

  // With two candidates neither keeps its literals, so they agree.
  const later = pick(() => "a", () => "b");
  const counts = pick((n: number) => 0, (n: number) => n + 1);
  assert(later() === "b" && counts(4) === 5, "two candidates widen");

  // Nor does a function literal in a conditional, whose branches would
  // otherwise disagree.
  const chosen = id(one > 5 ? () => 1 : () => 2);
  assert(chosen() === 2, "a conditional's branches widen");

  let copy = id(1);
  copy = 5;
  assert(copy === 5, "a let widens");
}
