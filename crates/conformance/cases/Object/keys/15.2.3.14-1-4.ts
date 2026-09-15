// test262: test/built-ins/Object/keys/15.2.3.14-1-4.js
// Object.keys(null) throws. The TypeError/Error distinction is erased by
// the port — there are no Error subclasses.

function main(): void {
  assertThrows((): void => {
    Object.keys(null);
  }, "Object.keys(null) throws");
}
