// A literal passed straight for a type parameter that is the call's result
// keeps its literal type, as in tsc: `id(1)` is `1`. So does the return of a
// function literal passed for a bare type parameter: `id(() => 42)` is
// `() => 42`. A `let` still widens what it copies.
function id<T>(x: T): T {
  return x;
}

function maybe<T>(x: T, keep: boolean): T | null {
  return keep ? x : null;
}

function run<A>(options: { a: A; b: (a: A) => void }): A {
  options.b(options.a);
  return options.a;
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

  const seen: number[] = [];
  const fromOption: () => 42 = run({
    a: () => {
      return 42;
    },
    b(a) {
      seen.push(a());
    },
  });
  assert(fromOption() === 42 && seen.join(",") === "42", "a sibling property");

  let copy = id(1);
  copy = 5;
  assert(copy === 5, "a let widens");
}
