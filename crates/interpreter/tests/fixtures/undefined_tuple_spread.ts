function copy(value: [number, string?]): [number, string?] {
  return [...value];
}

function prefix(value: [number, string?]): [boolean, number, string?] {
  return [true, ...value];
}

function suffix(value: [number, string?]): [number, string | boolean | undefined, boolean?] {
  return [...value, true];
}

function copyUnion(value: [number, string?]): [number, string?] | [boolean] {
  return [...value];
}

function unionLength(value: [number, string?] | [boolean] | null): 1 | 2 | undefined {
  return value?.length;
}

function main(): void {
  const short = copy([1]);
  assert(short.length === 1 && short[1] === undefined, "spread preserves missing tuple element");
  const full = copy([2, "two"]);
  assert(full.length === 2 && full[1] === "two", "spread preserves present tuple element");
  assert(prefix([3]).length === 2, "fixed prefix keeps optional spread cardinality");
  const shifted = suffix([4]);
  assert(shifted.length === 2 && shifted[1] === true, "suffix follows actual source length");
  assert(suffix([5, "five"])[2] === true, "suffix follows present optional element");
  assert(copyUnion([6]).length === 1, "tuple union context preserves cardinality");
  assert(unionLength([7]) === 1 && unionLength([8, "eight"]) === 2, "tuple union lengths remain literal types");
  assert(unionLength([true]) === 1 && unionLength(null) === undefined, "optional tuple union length");
}
