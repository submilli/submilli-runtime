// test262: test/built-ins/Array/prototype/with/index-bigger-or-eq-than-length.js
// Adapted: the standard's RangeError is the base Error here (no error
// subclasses yet); assertThrows checks only that it throws.

function main(): void {
  assertThrows((): void => {
    [0, 1, 2].with(3, 7);
  }, "with(3) on a 3-element array");

  assertThrows((): void => {
    [0, 1, 2].with(10, 7);
  }, "with(10) on a 3-element array");

  assertThrows((): void => {
    [0, 1, 2].with(9007199254740994, 7);
  }, "with(2 ** 53 + 2)");

  assertThrows((): void => {
    [0, 1, 2].with(Infinity, 7);
  }, "with(Infinity)");
}
