// Where two values are equal, an object member stays though the other side's
// object type is unrelated, since one object can have both shapes.
// expect-error: expected `null`, got `P | null`
// expect-error-count: 1
type P = { a: number };
type Q = { b: number };

function same(x: P | null, y: Q | null): boolean {
  if (x === y) {
    const none: null = x;
    return none === null;
  }
  return false;
}

function main(): void {
  const both = { a: 1, b: 2 };
  console.log(same(both, both));
}
