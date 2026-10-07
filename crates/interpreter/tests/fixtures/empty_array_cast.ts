// An empty `[]` cast to an array type takes its element type from the target,
// as it would from an annotation. The usual use is an empty `reduce` accumulator.
type Numbers = number[];
type MaybeNumbers = Numbers | null;

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

  const aliased = [] as MaybeNumbers;
  assert(aliased !== null && aliased.length === 0, "alias of a union with an alias");

  const viewed = [] as ReadonlyArray<number>;
  assert(viewed.length === 0, "ReadonlyArray");

  const angle = <string[]>[];
  angle.push("a");
  assert(angle[0] === "a", "angle-bracket cast");

  const counts = new Map<string, number[]>();
  const fallback = counts.get("missing") ?? ([] as number[]);
  for (const n of fallback) {
    assert(n !== n, "an empty fallback has no elements");
  }
  const chosen = counts.size > 0 ? [7] : ([] as number[]);
  assert(chosen.length === 0, "ternary branch");

  const either = [] as [number, number] | string[];
  assert(Array.isArray(either), "array member of a tuple-or-array union");

  const column = [] as number[] | string[];
  assert(column.length === 0, "first array member of a union of arrays");

  const branch = (counts.size > 0 ? [] : [4]) as number[];
  assert(branch[0] === 4, "an empty ternary branch inside the cast");

  const nested = [[], [5]] as number[][];
  nested[0].push(6);
  assert(JSON.stringify(nested) === "[[6],[5]]", "an empty array nested in the cast");

  const columns = (counts.size > 0 ? [] : ["x"]) as number[] | string[];
  assert(columns.length === 1, "an empty branch under a union of arrays");

  assert(orEmpty([] as string[]).length === 0, "argument position");
  assert(orEmpty<number>(null).length === 0, "generic element type");
}
