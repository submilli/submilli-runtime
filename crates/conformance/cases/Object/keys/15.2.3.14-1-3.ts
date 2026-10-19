// test262: test/built-ins/Object/keys/15.2.3.14-1-3.js
// Object.keys does not throw on a non-object first argument (string).

function main(): void {
  Object.keys("abc");
}
