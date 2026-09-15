// test262: test/built-ins/String/prototype/includes/return-false-with-out-of-bounds-position.js

function main(): void {
  const str = "The future is cool!";

  assertSameValue(
    str.includes("!", str.length + 1), false,
    'str.includes("!", str.length + 1) returns false',
  );

  assertSameValue(
    str.includes("!", 100), false,
    'str.includes("!", 100) returns false',
  );

  assertSameValue(
    str.includes("!", Infinity), false,
    'str.includes("!", Infinity) returns false',
  );

  assertSameValue(
    str.includes("!", str.length), false,
    'str.includes("!", str.length) returns false',
  );
}
