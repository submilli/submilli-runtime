// test262: test/built-ins/Boolean/S9.2_A3_T1.js
// Adapted: Boolean(value) is spelled !!value; Boolean is not callable here.

function main(): void {
  assertSameValue(!!(true), true, 'Boolean(true) must return true');
  assertSameValue(!!(false), false, 'Boolean(false) must return false');
}
