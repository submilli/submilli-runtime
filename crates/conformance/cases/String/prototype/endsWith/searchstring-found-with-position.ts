// test262: test/built-ins/String/prototype/endsWith/searchstring-found-with-position.js

function main(): void {
  const str = "The future is cool!";

  assert(
    str.endsWith("The future", 10),
    'str.endsWith("The future", 10) === true',
  );
  assert(
    str.endsWith("future", 10),
    'str.endsWith("future", 10) === true',
  );
  assert(
    str.endsWith(" is cool!", str.length),
    'str.endsWith(" is cool!", str.length) === true',
  );
}
