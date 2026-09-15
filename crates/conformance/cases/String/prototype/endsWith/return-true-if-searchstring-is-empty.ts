// test262: test/built-ins/String/prototype/endsWith/return-true-if-searchstring-is-empty.js

function main(): void {
  const str = "The future is cool!";

  assert(str.endsWith(""), 'str.endsWith("") returns true');
  assert(str.endsWith("", str.length), 'str.endsWith("", str.length) returns true');
  assert(str.endsWith("", Infinity), 'str.endsWith("", Infinity) returns true');
  assert(str.endsWith("", -1), 'str.endsWith("", -1) returns true');
  assert(str.endsWith("", -Infinity), 'str.endsWith("", -Infinity) returns true');
}
