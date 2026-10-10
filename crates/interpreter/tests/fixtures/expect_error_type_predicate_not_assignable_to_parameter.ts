// A type predicate's type must be assignable to its parameter's type, as tsc
// requires, for a declared function, an arrow and a callback: `x is boolean`
// on a `string | number` can never hold, and a wider type is no narrowing.
// expect-error: a type predicate's type must be assignable to its parameter's type: `boolean` is not a `number | string`
// expect-error: a type predicate's type must be assignable to its parameter's type: `number | string` is not a `string`
// expect-error: a type predicate's type must be assignable to its parameter's type: `boolean` is not a `number | string`
// expect-error: a type predicate's type must be assignable to its parameter's type: `boolean` is not a `number | string`
// expect-error-count: 4
function isFlag(x: string | number): x is boolean {
  return false;
}

function isAny(x: string): x is string | number {
  return true;
}

function isText(x: string | number): x is string {
  return typeof x === "string";
}

function main(): void {
  const isFlagArrow = (x: string | number): x is boolean => false;
  const values: (string | number)[] = ["a", 1];
  const flags = values.filter((x): x is boolean => typeof x === "boolean");
  console.log(isFlag("a"), isAny("b"), isText(1), isFlagArrow(2), flags.length);
}
