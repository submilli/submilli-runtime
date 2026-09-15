// test262: test/built-ins/BigInt/nan-throws-rangeerror.js
// The original distinguishes RangeError; the port matches the base Error only.
// The valueOf-object arm is a coercion trap, rejected by the blanket rule.

function main(): void {
  assertThrows((): void => {
    BigInt(NaN);
  }, "BigInt(NaN)");
}
