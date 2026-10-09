// test262: test/built-ins/Boolean/S15.6.1.1_A1_T3.js
// Adapted: Boolean(value) uses !!value; the Boolean constructor is unsupported.

function main(): void {
  assertSameValue(typeof !!(undefined), "boolean");
  assertSameValue(!!("0"), true, 'Boolean("0") must return true');
  assertSameValue(!!("-1"), true, 'Boolean("-1") must return true');
  assertSameValue(!!("1"), true, 'Boolean("1") must return true');
  assertSameValue(!!("false"), true, 'Boolean("false") must return true');
  assertSameValue(!!("true"), true, 'Boolean("true") must return true');
}
