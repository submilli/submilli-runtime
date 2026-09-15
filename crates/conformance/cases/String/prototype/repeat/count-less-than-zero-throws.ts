// test262: test/built-ins/String/prototype/repeat/count-less-than-zero-throws.js
// RangeError is erased to the base Error by the port.

function main(): void {
  assertThrows((): void => {
    "".repeat(-1);
  });

  assertThrows((): void => {
    "".repeat(-Infinity);
  });
}
