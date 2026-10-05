// An empty `[]` cast to an array type takes its element type from the target,
// as it would from an annotation. The usual use is an empty `reduce` accumulator.
function orEmpty<T>(xs: T[] | null): T[] {
  return xs !== null ? xs : ([] as T[]);
}

function main(): void {
  const numbers = [] as number[];
  numbers.push(3);
  assert(numbers.join(",") === "3", "pushes onto the cast array");

  const lengths = ["a", "bb"].reduce((acc, word) => {
    acc.push(word.length);
    return acc;
  }, [] as number[]);
  assert(lengths.join(",") === "1,2", "reduce accumulator");

  const names = [] as readonly string[];
  assert(names.length === 0, "readonly target");

  const maybe = [] as number[] | null;
  assert(maybe !== null && maybe.length === 0, "array member of a union target");

  const parenthesized = ([]) as Array<number>;
  assert(parenthesized.length === 0, "parenthesized operand, generic spelling");

  const pairs = [] as [number, string][];
  pairs.push([1, "one"]);
  assert(pairs[0][1] === "one", "tuple elements");

  assert(orEmpty([] as string[]).length === 0, "argument position");
  assert(orEmpty<number>(null).length === 0, "generic element type");
}
