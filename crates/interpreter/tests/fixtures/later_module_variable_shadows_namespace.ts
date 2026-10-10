// A later module-level `const` named like a built-in namespace shadows it in a
// function body above it, as in JavaScript. (`export` makes the file a module,
// where TypeScript lets it shadow the global.)

const floorOf = (x: number): number => Math.floor(x);
const Math: { floor: (x: number) => number } = {
  floor: (x: number): number => x * 10,
};
assert(floorOf(2.5) === 25, "the user's `Math`, not the built-in");

const stringifyOne = (): string => JSON.stringify(1);
const readJson = (): string => {
  const json = JSON;
  return json.stringify(2);
};
const JSON: { stringify: (x: number) => string } = {
  stringify: (x: number): string => "u" + String(x),
};
assert(stringifyOne() === "u1", "the user's `JSON` in a call");
assert(readJson() === "u2", "the user's `JSON` as a value");

export function main(): void {}
