// A readonly array is not a mutable one, also where a generic parameter's
// type parameter would take it: the callee could write to an array the caller
// declared readonly. tsc infers `T` through the array and rejects the argument.
// expect-error: expected `("off" | "on")[]`, got `readonly Mode[]`
// expect-error: expected `T[]`, got `readonly number[]`
// expect-error: expected `T[]`, got `readonly string[]`
// expect-error: parameter `copy`: expected `readonly number[]`, got `number[]`
// expect-error-count: 4
type Mode = "on" | "off";

function oneOrMany<T>(one: T | T[], fallback: T): T | T[] {
  return one;
}

function push<T>(xs: T[], x: T): void {
  xs.push(x);
}

function eachView<T>(xs: T[], f: (view: readonly T[]) => void): void {}

function main(): void {
  const rom: readonly Mode[] = ["on"];
  const many = oneOrMany(rom, "on");
  const nums: readonly number[] = [1];
  push(nums, 2);
  const tagged: readonly string[] = ["a"];
  push(tagged, "b");
  eachView([1], (copy: number[]) => {});
  eachView([1], (view: readonly number[]) => {});
  const ok = oneOrMany(["on"], "off");
  push([1], 2);
}
