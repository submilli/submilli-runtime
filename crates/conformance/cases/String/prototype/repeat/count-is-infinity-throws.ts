// test262: test/built-ins/String/prototype/repeat/count-is-infinity-throws.js
// RangeError is erased to the base Error by the port.

function main(): void {
  assertThrows((): void => {
    "".repeat(Infinity);
  });
}
