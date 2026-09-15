// test262: test/built-ins/String/fromCodePoint/number-is-out-of-range.js
// RangeError is erased to the base Error by the port.

function main(): void {
  assertThrows((): void => {
    String.fromCodePoint(-1);
  });

  assertThrows((): void => {
    String.fromCodePoint(1, -1);
  });

  assertThrows((): void => {
    String.fromCodePoint(1114112);
  });

  assertThrows((): void => {
    String.fromCodePoint(Infinity);
  });
}
