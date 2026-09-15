// test262: test/built-ins/String/prototype/repeat/count-is-infinity-throws.js
// expect-fail: repeat(Infinity) should throw a RangeError; the count saturates and the empty receiver returns "" without throwing
// RangeError is erased to the base Error by the port.

function main(): void {
  assertThrows((): void => {
    "".repeat(Infinity);
  });
}
