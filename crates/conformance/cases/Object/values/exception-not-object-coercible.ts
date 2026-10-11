// test262: test/built-ins/Object/values/exception-not-object-coercible.js
// The TypeError/Error distinction is erased by the port (assertThrows
// matches the base Error).

function main(): void {
  assertThrows((): void => {
    Object.values(null);
  }, "Object.values(null) throws");

  assertThrows((): void => {
    Object.values(undefined);
  }, "Object.values(undefined) throws");
}
