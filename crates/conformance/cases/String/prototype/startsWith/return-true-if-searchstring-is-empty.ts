// test262: test/built-ins/String/prototype/startsWith/return-true-if-searchstring-is-empty.js

function main(): void {
  const str = "The future is cool!";

  assert(str.startsWith(""), 'str.startsWith("") returns true');
  assert(str.startsWith("", str.length), 'str.startsWith("", str.length) returns true');
  assert(str.startsWith(""), 'str.startsWith("") returns true');
  assert(str.startsWith("", Infinity), 'str.startsWith("", Infinity) returns true');
}
