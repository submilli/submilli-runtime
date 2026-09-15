// test262: test/built-ins/String/prototype/substring/S15.5.4.15_A2_T10.js
// `new String(...)` wrapper object replaced by the plain string.

function main(): void {
  assertSameValue(
    "this_is_a_string object".substring(0, 8),
    "this_is_",
    "substring(0, 8)",
  );
}
