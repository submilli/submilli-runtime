// test262: test/built-ins/TypedArray/prototype/with/index-bigger-or-eq-than-length.js
// Instantiated at Uint8Array. RangeError distinction is erased: assertThrows
// matches the base Error. The host-fn port raises a catchable Error on
// out-of-range `with`, matching the standard's RangeError (modulo subtype).

function main(): void {
  const arr: Uint8Array = new Uint8Array([0, 1, 2]);

  assertThrows((): void => {
    arr.with(3, 7);
  }, "with(3, 7)");

  assertThrows((): void => {
    arr.with(10, 7);
  }, "with(10, 7)");

  assertThrows((): void => {
    arr.with(Math.pow(2, 53) + 2, 7);
  }, "with(2**53 + 2, 7)");

  assertThrows((): void => {
    arr.with(Infinity, 7);
  }, "with(Infinity, 7)");
}
