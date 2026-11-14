// A generic function type returning `string` is not assignable to one
// returning `number`.
// expect-error: expected `<U>(arg0: U) => number`, got `<U>(arg0: U) => string`
// expect-error-count: 1
function main(): void {
  const actual: <U>(arg: U) => string = <U,>(arg: U): string => "x";
  const expected: <U>(arg: U) => number = actual;
}
