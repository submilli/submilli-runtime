// test262: test/built-ins/Infinity/S15.1.1.2_A2_T2.js
// expect-error: cannot assign to const binding `Infinity`
//
// `Infinity` is a prelude constant, so writing it is a compile error. The standard
// makes the global non-writable instead: a sloppy-mode write is silently
// ignored (this test's `noStrict` flag) and a strict-mode write throws.

function main(): void {
  Infinity = true;
  assert(typeof Infinity !== "boolean", 'The value of typeof(Infinity) is not "boolean"');
}
