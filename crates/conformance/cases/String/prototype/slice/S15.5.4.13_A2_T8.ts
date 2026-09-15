// test262: test/built-ins/String/prototype/slice/S15.5.4.13_A2_T8.js
// `new String(...)` wrapper object replaced by the plain string.

function main(): void {
  const str = "this is a string object";
  assertSameValue(
    str.slice(str.length + 1, 0),
    "",
    "slice(length + 1, 0) returns the empty string",
  );
}
