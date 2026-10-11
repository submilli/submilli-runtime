// test262: test/built-ins/Object/hasOwn/toobject_undefined.js
// Object.hasOwn(undefined, ...) throws. The TypeError/Error distinction is
// erased by the port (assertThrows matches the base Error).

function main(): void {
  assertThrows((): void => {
    Object.hasOwn(undefined, "foo");
  }, "Object.hasOwn(undefined, 'foo') throws");
}
