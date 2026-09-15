// test262: test/built-ins/String/prototype/repeat/repeat-string-n-times.js
// The original builds the 10000-dot expected string one += at a time; that
// quadratic loop exhausts the runner's fuel budget, so the expected string
// is built with padStart as an independent oracle.

function main(): void {
  const str = "abc";
  assertSameValue(str.repeat(1), str, "str.repeat(1) === str");
  assertSameValue(str.repeat(3), "abcabcabc", 'str.repeat(3) === "abcabcabc"');

  const count = 10000;
  const expected = "".padStart(count, ".");

  assertSameValue(".".repeat(count), expected);
}
