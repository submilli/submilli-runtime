// test262: test/built-ins/Error/isError/non-error-objects.js
//
// The arms passing the constructors themselves (`Error.isError(Error)`, …)
// are dropped: a class is not a value here. `function () {}` becomes an
// arrow function, and `[]` gets an element type.

function main(): void {
  const emptyArray: number[] = [];
  assertSameValue(Error.isError({}), false);
  assertSameValue(Error.isError(emptyArray), false);
  assertSameValue(Error.isError((): void => {}), false);
  assertSameValue(Error.isError(/a/g), false);
}
