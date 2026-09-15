// test262: test/built-ins/Boolean/prototype/toString/S15.6.4.2_A1_T2.js
// expect-error: method `toString` expects 0 argument(s), got 1
//
// JS ignores extra arguments to toString; our typed signatures reject them
// at compile time (same divergence as TypeScript itself).

function main(): void {
  assertSameValue(false.toString(true), "false", 'false.toString(true) must return "false"');
  assertSameValue(true.toString(false), "true", 'true.toString(false) must return "true"');
}
