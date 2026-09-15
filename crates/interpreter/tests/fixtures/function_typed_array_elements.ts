// Arrays and tuples whose elements are function-typed, spelled inline (not
// through a type alias). The alias spelling has always worked; the inline
// forms used to be rejected by four typechecker guards.
type Fn = (n: number) => number;

class Pipeline {
  private steps: Array<(n: number) => number> = [];
  add(step: (n: number) => number): void {
    this.steps.push(step);
  }
  run(n: number): number {
    let acc = n;
    for (const step of this.steps) {
      acc = step(acc);
    }
    return acc;
  }
}

function first(fns: Fn[], n: number): number {
  return fns.length > 0 ? fns[0](n) : n;
}

function main(): void {
  const bump = 10;
  const inline: Array<(n: number) => number> = [
    (n: number) => n + bump,
    (n: number) => n * 2,
  ];
  assert(inline[0](1) === 11, "captured closure through an inline element type");
  assert(inline[1](3) === 6, "second inline element");
  assert(first(inline, 4) === 14, "inline array passed as an alias-typed param");

  const mapped: number[] = inline.map((f: (n: number) => number) => f(2));
  assert(mapped[0] === 12 && mapped[1] === 4, "map over a function-typed array");

  const inferred = [(n: number) => n + 100, (n: number) => n + 200];
  assert(inferred[0](1) === 101, "unannotated array literal of arrows");
  assert(inferred[1](1) === 201, "second unannotated element");

  const tup: [(n: number) => number, string] = [(n: number) => n * 5, "s"];
  assert(tup[0](2) === 10, "function-typed tuple element");
  assert(tup[1] === "s", "tuple keeps its other elements");

  const nested: Array<Array<(n: number) => number>> = [[(n: number) => n - 1]];
  assert(nested[0][0](5) === 4, "nested function-typed array");

  const p = new Pipeline();
  p.add((n: number) => n + 1);
  p.add((n: number) => n * 3);
  assert(p.run(2) === 9, "class field typed Array<(n) => n>");

  const empty: Fn[] = [];
  empty.push((n: number) => n + 1);
  assert(empty[0](1) === 2, "push into an empty function-typed array");

  // Elements sharing captured mutable state.
  let count = 0;
  const bumps: Array<() => number> = [];
  bumps.push(() => {
    count = count + 1;
    return count;
  });
  bumps.push(() => {
    count = count + 10;
    return count;
  });
  assert(bumps[0]() === 1 && bumps[1]() === 11, "closures in an array share captured state");

  // The array methods, over a function-typed element.
  assert(
    inline.filter((f: (n: number) => number) => f(1) > 5).length === 1,
    "filter over a function array",
  );
  assert(
    inline.reduce((acc: number, f: (n: number) => number) => acc + f(2), 0) === 16,
    "reduce over a function array",
  );
  assert(inline.indexOf(inline[0]) === 0, "elements compare by identity");

  // A function type as a generic argument, and as an interface field.
  const byName = new Map<string, (n: number) => number>();
  byName.set("inc", (n: number) => n + 1);
  assert(byName.get("inc")!(1) === 2, "Map value typed as a function");

  const spread: Array<(n: number) => number> = [...inline, (n: number) => n - 1];
  assert(spread.length === 3 && spread[2](5) === 4, "spread of a function array");

  // Produced by `map`, and a zero-arg void element.
  const made: Array<(n: number) => number> = [1, 2].map(
    (k: number) => (n: number) => n + k,
  );
  assert(made[0](0) === 1 && made[1](0) === 2, "map producing a function array");
  const sinks: Array<() => void> = [];
  let hit = 0;
  sinks.push(() => {
    hit = hit + 1;
  });
  sinks[0]();
  assert(hit === 1, "zero-arg void element");
}
