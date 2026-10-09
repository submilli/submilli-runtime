// test262: test/built-ins/Boolean/S15.6.1.1_A1_T2.js
// Adapted: Boolean(value) uses !!value; the Boolean constructor is unsupported.

function main(): void {
  assertSameValue(typeof !!(undefined), "boolean");
  assertSameValue(!!(0), false, 'Boolean(0) must return false');
  assertSameValue(!!(-1), true, 'Boolean(-1) must return true');
  assertSameValue(!!(-Infinity), true, 'Boolean(-Infinity) must return true');
  assertSameValue(!!(NaN), false, 'Boolean(NaN) must return false');
}
