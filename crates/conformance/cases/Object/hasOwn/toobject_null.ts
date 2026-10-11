// test262: test/built-ins/Object/hasOwn/toobject_null.js
// Object.hasOwn(null, ...) throws. The TypeError/Error distinction is
// erased by the port (assertThrows matches the base Error).

function main(): void {
  assertThrows((): void => {
    Object.hasOwn(null, "foo");
  }, "Object.hasOwn(null, 'foo') throws");
}
