// test262: test/built-ins/Object/keys/15.2.3.14-1-1.js
// Object.keys does not throw on a non-object first argument (number).

function main(): void {
  Object.keys(0);
}
