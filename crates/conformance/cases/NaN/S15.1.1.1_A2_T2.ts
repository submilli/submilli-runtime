// test262: test/built-ins/NaN/S15.1.1.1_A2_T2.js
// expect-error: cannot assign to const binding `NaN`
//
// `NaN` is a prelude constant, so writing it is a compile error. The standard
// makes the global non-writable instead: a sloppy-mode write is silently
// ignored (this test's `noStrict` flag) and a strict-mode write throws.

function main(): void {
  NaN = true;
  assert(typeof NaN !== "boolean", 'The value of typeof(NaN) is not "boolean"');
}
