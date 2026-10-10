// A function body may use a module-level `let`/`const` declared below it, as in
// TypeScript: it only runs once called. The variable's type comes from what its
// declaration states: an annotation, a literal, or an arrow with written types.

const fromBlock: (() => number)[] = [];
let readLater: () => number = () => later;
const shout = (): string => word + "!";
const viaIife = ((): (() => number) => {
  return (): number => annotated;
})();
const factorial = (n: number): number => (n <= 1 ? 1 : n * factorial(n - 1));
const isEven = function (n: number): boolean {
  return n === 0 ? true : isOdd(n - 1);
};
const isOdd = (n: number): boolean => (n === 0 ? false : isEven(n - 1));
const bump = (): void => {
  counter += 1;
  counter++;
  total = total + counter;
};
const readLimit = (): number => config.limit;
const viaObjectMethod = {
  read(): number {
    return later;
  },
};
{
  const inBlock = (): number => later;
  fromBlock.push((): number => inBlock() * 2);
}

let later = 5;
const word = "hi";
const annotated: number = 3;
let counter = 0;
let total: number = 0;
const config: { limit: number } = { limit: 7 };

assert(readLater() === 5, "a later `let` with a literal initializer");
assert(shout() === "hi!", "a later `const` with a literal initializer");
assert(viaIife() === 3, "a later annotated `const`, through a returned arrow");
assert(factorial(5) === 120, "an arrow calls itself through its own `const`");
assert(isEven(10) && isOdd(7), "a function expression and an arrow call each other");
bump();
bump();
assert(counter === 4 && total === 6, "writes a later `let`");
assert(readLimit() === 7, "reads a field of a later annotated `const`");
assert(viaObjectMethod.read() === 5, "an object literal method");
assert(fromBlock[0]() === 10, "an arrow in a top-level block");
later = 6;
assert(readLater() === 6, "sees the latest write");

// The annotation is resolved where the declaration is, not where it's used.
const width = 10;
const padded = (): number => {
  const width = "local";
  return margin + width.length;
};
let margin: typeof width = 10;
assert(padded() === 15, "a `typeof` annotation names the module-level `width`");

function main(): void {}
