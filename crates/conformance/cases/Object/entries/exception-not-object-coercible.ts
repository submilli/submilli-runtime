// test262: test/built-ins/Object/entries/exception-not-object-coercible.js
// The TypeError/Error distinction is erased by the port (assertThrows
// matches the base Error).

function main(): void {
  assertThrows((): void => {
    Object.entries(null);
  }, "Object.entries(null) throws");

  assertThrows((): void => {
    Object.entries(undefined);
  }, "Object.entries(undefined) throws");
}
