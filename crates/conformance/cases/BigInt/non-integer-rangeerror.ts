// test262: test/built-ins/BigInt/non-integer-rangeerror.js
// The original distinguishes RangeError; the port matches the base Error only.

function main(): void {
  assertThrows((): void => {
    BigInt(0.00005);
  }, "BigInt(0.00005)");

  assertThrows((): void => {
    BigInt(-0.00005);
  }, "BigInt(-0.00005)");

  assertThrows((): void => {
    BigInt(0.1);
  }, "BigInt(.1)");

  assertThrows((): void => {
    BigInt(-0.1);
  }, "BigInt(-.1)");

  assertThrows((): void => {
    BigInt(1.1);
  }, "BigInt(1.1)");

  assertThrows((): void => {
    BigInt(-1.1);
  }, "BigInt(-1.1)");

  assertThrows((): void => {
    BigInt(Number.MIN_VALUE);
  }, "BigInt(Number.MIN_VALUE)");
}
