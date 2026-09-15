// test262: test/built-ins/String/prototype/includes/return-true-if-searchstring-is-empty.js

function main(): void {
  const str = "The future is cool!";

  assert(
    str.includes("", str.length),
    'str.includes("", str.length) returns true',
  );

  assert(str.includes(""), 'str.includes("") returns true');

  assert(
    str.includes("", Infinity),
    'str.includes("", Infinity) returns true',
  );
}
