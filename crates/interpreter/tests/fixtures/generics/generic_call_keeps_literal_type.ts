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

type Mode = "on" | "off";

function first<T>(xs: T[]): T {
  return xs[0];
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

  // A function literal's inferred return type widens a call's literal
  // result, as it widens a literal.
  let letter = () => id("a");
  letter = () => "b";
  const lengths = [1, 2].map(() => id(0));
  lengths.push(4);
  assert(letter() === "b" && lengths.join(",") === "0,0,4", "a returned call widens");

  // So does a fresh literal reached through a `const`, whether the `const`
  // holds the literal or a call that kept it.
  const label = "fixed";
  const flag = true;
  const labels = [1, 2].map(() => label);
  labels.push("other");
  const flags = [1].map(() => flag);
  flags.push(false);
  const blocks = [1].map(() => {
    const local = "blk";
    return local;
  });
  blocks.push("more");
  const kept1 = id(1);
  let counter = () => kept1;
  counter = () => 2;
  assert(labels.join(",") === "fixed,fixed,other" && flags.join(",") === "true,false", "a const's literal widens");
  assert(blocks.join(",") === "blk,more" && counter() === 2, "through a block and a kept call");

  // A literal type the call declares, rather than takes from a fresh
  // argument, stays.
  const modes: Mode[] = ["on", "off"];
  const lookup = new Map<string, Mode>([["x", "off"]]);
  const firstMode = () => first(modes);
  const found = () => modes.find((mode) => mode === "off") ?? null;
  const stored = () => lookup.get("x") ?? null;
  const mode: Mode = firstMode();
  const maybeMode: Mode | null = found() ?? stored();
  const echoed: Mode[] = modes.map((each) => id(each));
  assert(mode === "on" && maybeMode === "off", "a declared literal type stays");
  assert(echoed.join(",") === "on,off", "through a call that keeps it");

  let copy = id(1);
  copy = 5;
  assert(copy === 5, "a let widens");
}
