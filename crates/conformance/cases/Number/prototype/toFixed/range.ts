// test262: test/built-ins/Number/prototype/toFixed/range.js
//
// test262 expects RangeError; the port matches the base Error (no error
// subclasses), which is the distinction erased noted in README.md.

function main(): void {
  assertSameValue((3).toFixed(-0), "3");
  assertThrows((): void => {
    (3).toFixed(-1);
  });

  assertSameValue((3).toFixed(100), "3.0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000");
  assertThrows((): void => {
    (3).toFixed(101);
  });
}
