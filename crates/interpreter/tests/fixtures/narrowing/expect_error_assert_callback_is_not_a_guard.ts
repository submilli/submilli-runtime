// An assertion passed as a callback is not a type guard, so `filter`, `find`
// and `every` keep the element type, as tsc does.
// expect-error-count: 3
// expect-error: expected `string[]`, got `(number | string)[]`
// expect-error: expected `string | undefined`, got `number | string | undefined`
function assertIsString(x: unknown): asserts x is string {
  if (typeof x !== "string") throw new Error("not a string");
}

function main(): void {
  const xs: (string | number)[] = ["a"];
  const strings: string[] = xs.filter(assertIsString);
  const found: string | undefined = xs.find(assertIsString);
  if (xs.every(assertIsString)) {
    const all: string[] = xs;
  }
}
