// test262: test/built-ins/String/prototype/slice/S15.5.4.13_A2_T2.js
// `new String(...)` wrapper object replaced by the plain string.

function main(): void {
  assertSameValue(
    "this is a string object".slice(NaN, Infinity),
    "this is a string object",
    "slice(NaN, Infinity) returns the whole string",
  );
}
