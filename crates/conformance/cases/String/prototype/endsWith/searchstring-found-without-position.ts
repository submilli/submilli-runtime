// test262: test/built-ins/String/prototype/endsWith/searchstring-found-without-position.js

function main(): void {
  const str = "The future is cool!";

  assert(str.endsWith("cool!"), 'str.endsWith("cool!") === true');
  assert(str.endsWith("!"), 'str.endsWith("!") === true');
  assert(str.endsWith(str), "str.endsWith(str) === true");
}
