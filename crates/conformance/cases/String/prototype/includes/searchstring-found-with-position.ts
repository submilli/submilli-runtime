// test262: test/built-ins/String/prototype/includes/searchstring-found-with-position.js

function main(): void {
  const str = "The future is cool!";

  assert(
    str.includes("The future", 0),
    'Returns true for str.includes("The future", 0)',
  );
  assert(str.includes(" is ", 1), 'Returns true for str.includes(" is ", 1)');
  assert(str.includes("cool!", 10), 'Returns true for str.includes("cool!", 10)');
}
