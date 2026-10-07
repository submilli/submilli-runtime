// A literal's own type never narrows what a binding can hold, and the right
// side of `a || b` keeps its context when `a` decides the result.

function addString(xs: (string | number)[]): void {
  xs.push("y");
}

interface Named {
  name(): string;
}

function pick(flag: boolean): boolean {
  return flag;
}

function main(): void {
  const pair: [number, string] = [1, "a"];
  const keptPair: [number, string] = pair || [2, "b"];
  const nums: number[] = [1];
  const keptNums: number[] = nums || [];
  const double = (x: number): number => x * 2;
  const keptFn: (x: number) => number = double || ((x) => x);
  assert(keptPair[1] === "a" && keptNums.length === 1 && keptFn(3) === 6, "`||` gives its right side context");
  const named: Named = { name: (): string => "n" };
  const keptNamed: Named = named || { name: (): string => "z" };
  assert(keptNamed.name() === "n", "an object literal on the right takes an interface as context");

  let mixed: (string | number)[] = [1, 2];
  mixed = [3];
  mixed[0] = "y";
  addString(mixed);
  assert(mixed[0] === "y" && mixed[1] === "y", "an array write keeps the declared element type");

  let either: (string | number)[] = [];
  either = pick(true) ? [1] : ["x"];
  either.push("k");
  assert(either.length === 2, "a conditional array write keeps the declared element type");

  let maybe: (string | number)[] | null = null;
  maybe = [1];
  maybe.push("s");
  assert(maybe.length === 2, "a nullable array binding takes its declared elements after a write");
  console.log("ok");
}
