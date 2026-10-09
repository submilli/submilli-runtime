// test262: test/built-ins/Boolean/S15.6.1.1_A1_T2.js
// Adapted: Boolean(value) is spelled !!value; Boolean is not callable here.

function main(): void {
  assertSameValue(typeof !!(0), "boolean", 'The value of `typeof Boolean(0)` is expected to be "boolean"');
  assertSameValue(!!(0), false, 'Boolean(0) must return false');
  assertSameValue(typeof !!(-1), "boolean", 'The value of `typeof Boolean(-1)` is expected to be "boolean"');
  assertSameValue(!!(-1), true, 'Boolean(-1) must return true');

  assertSameValue(
    typeof !!(-Infinity),
    "boolean",
    'The value of `typeof Boolean(-Infinity)` is expected to be "boolean"'
  );

  assertSameValue(!!(-Infinity), true, 'Boolean(-Infinity) must return true');
  assertSameValue(typeof !!(NaN), "boolean", 'The value of `typeof Boolean(NaN)` is expected to be "boolean"');
  assertSameValue(!!(NaN), false, 'Boolean(NaN) must return false');
}
