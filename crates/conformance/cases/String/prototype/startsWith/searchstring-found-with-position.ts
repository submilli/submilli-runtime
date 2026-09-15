// test262: test/built-ins/String/prototype/startsWith/searchstring-found-with-position.js

function main(): void {
  const str = "The future is cool!";

  assert(
    str.startsWith("The future", 0),
    'str.startsWith("The future", 0) === true',
  );
  assert(
    str.startsWith("future", 4),
    'str.startsWith("future", 4) === true',
  );
  assert(
    str.startsWith(" is cool!", 10),
    'str.startsWith(" is cool!", 10) === true',
  );
}
