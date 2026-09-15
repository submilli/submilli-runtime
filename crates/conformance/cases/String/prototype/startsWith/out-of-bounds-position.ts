// test262: test/built-ins/String/prototype/startsWith/out-of-bounds-position.js

function main(): void {
  const str = "The future is cool!";

  assertSameValue(
    str.startsWith("!", str.length), false,
    'str.startsWith("!", str.length) returns false',
  );

  assertSameValue(
    str.startsWith("!", 100), false,
    'str.startsWith("!", 100) returns false',
  );

  assertSameValue(
    str.startsWith("!", Infinity), false,
    'str.startsWith("!", Infinity) returns false',
  );

  assert(
    str.startsWith("The future", -1),
    "position argument < 0 will search from the start of the string (-1)",
  );

  assert(
    str.startsWith("The future", -Infinity),
    "position argument < 0 will search from the start of the string (-Infinity)",
  );
}
