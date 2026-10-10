// test262: test/built-ins/Object/keys/15.2.3.14-1-5.js
// Object.keys(undefined) throws. The TypeError/Error distinction is erased
// by the port (assertThrows matches the base Error).

function main(): void {
  assertThrows((): void => {
    Object.keys(undefined);
  }, "Object.keys(undefined) throws");
}
