// test262: test/built-ins/RegExp/S15.10.2.13_A1_T1.js
// expect-fail: the empty character class [] is valid in ECMA-262 (matches nothing); the engine rejects it at compile time with "unclosed character class"

function main(): void {
  assertSameValue(
    /[]a/.test("\u0000a\u0000a"),
    false,
    "[] never matches, so []a cannot match",
  );
}
