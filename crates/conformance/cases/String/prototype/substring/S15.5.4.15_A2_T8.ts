// test262: test/built-ins/String/prototype/substring/S15.5.4.15_A2_T8.js
// `new String(...)` wrapper object replaced by the plain string.

function main(): void {
  const str = "this is a string object";
  assertSameValue(
    str.substring(str.length + 1, 0),
    "this is a string object",
    "substring swaps start and end when start > end",
  );
}
