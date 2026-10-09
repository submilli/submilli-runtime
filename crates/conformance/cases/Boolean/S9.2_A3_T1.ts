// test262: test/built-ins/Boolean/S9.2_A3_T1.js
// Adapted: Boolean(value) uses !!value; the Boolean constructor is unsupported.

function main(): void {
  assertSameValue(typeof !!(undefined), "boolean");
  assertSameValue(!!(true), true, 'Boolean(true) must return true');
  assertSameValue(!!(false), false, 'Boolean(false) must return false');
}
