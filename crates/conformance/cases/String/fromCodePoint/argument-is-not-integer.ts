// test262: test/built-ins/String/fromCodePoint/argument-is-not-integer.js
// Numeric rows only; the string-argument rows ('3.14', '_1', undefined)
// are rejected by the type system. RangeError is erased to the base Error.

function main(): void {
  assertThrows((): void => {
    String.fromCodePoint(3.14);
  });

  assertThrows((): void => {
    String.fromCodePoint(42, 3.14);
  });
}
