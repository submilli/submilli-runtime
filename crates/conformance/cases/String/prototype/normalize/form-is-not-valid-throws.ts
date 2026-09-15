// test262: test/built-ins/String/prototype/normalize/form-is-not-valid-throws.js
// The `normalize(null)` row is rejected by the type system; RangeError is
// erased to the base Error.

function main(): void {
  assertThrows((): void => {
    "foo".normalize("bar");
  });

  assertThrows((): void => {
    "foo".normalize("NFC1");
  });
}
