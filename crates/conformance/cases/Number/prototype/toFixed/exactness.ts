// test262: test/built-ins/Number/prototype/toFixed/exactness.js

function main(): void {
  // Test from a note in the specification
  assertSameValue((1000000000000000128).toString(), "1000000000000000100");
  assertSameValue((1000000000000000128).toFixed(0), "1000000000000000128");
}
