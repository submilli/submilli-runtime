// A `filter`, `find` or `findLast` callback whose type guard asserts a class
// or an interface with methods narrows the result to it, as in tsc: each kept
// element is checked to be one when the result is built.
interface Named {
  name(): string;
}

class Cat {
  constructor(public n: string) {}
  name(): string {
    return this.n;
  }
}

function main(): void {
  const named: (Named | number)[] = [new Cat("a"), 2, new Cat("b")];
  const onlyNamed = named.filter((x): x is Named => typeof x !== "number");
  assert(onlyNamed.map((y) => y.name()).join(",") === "a,b", "an interface with methods");

  const cats: (Cat | number)[] = [1, new Cat("c")];
  const onlyCats = cats.filter((x): x is Cat => typeof x !== "number");
  assert(onlyCats.length === 1 && onlyCats[0].n === "c", "a class");
  const first = cats.find((x): x is Cat => typeof x !== "number");
  assert(first?.n === "c", "find");
  const last = cats.findLast((x): x is Cat => typeof x !== "number");
  assert(last?.name() === "c", "findLast");
}
