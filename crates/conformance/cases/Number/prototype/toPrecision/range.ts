// test262: test/built-ins/Number/prototype/toPrecision/range.js
//
// test262 expects RangeError; the port matches the base Error (no error
// subclasses), which is the distinction erased noted in README.md.

function main(): void {
  assertSameValue((3).toPrecision(1), "3");
  assertThrows((): void => {
    (3).toPrecision(0);
  });
  assertThrows((): void => {
    (3).toPrecision(-10);
  });

  assertSameValue((3).toPrecision(100), "3.000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000");
  assertThrows((): void => {
    (3).toPrecision(101);
  });
}
