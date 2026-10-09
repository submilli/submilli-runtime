// test262: test/built-ins/Boolean/S15.6.1.1_A1_T3.js
// Adapted: Boolean(value) is spelled !!value; Boolean is not callable here.

function main(): void {
  assertSameValue(typeof !!("0"), "boolean", 'The value of `typeof Boolean("0")` is expected to be "boolean"');
  assertSameValue(!!("0"), true, 'Boolean("0") must return true');
  assertSameValue(typeof !!("-1"), "boolean", 'The value of `typeof Boolean("-1")` is expected to be "boolean"');
  assertSameValue(!!("-1"), true, 'Boolean("-1") must return true');
  assertSameValue(typeof !!("1"), "boolean", 'The value of `typeof Boolean("1")` is expected to be "boolean"');
  assertSameValue(!!("1"), true, 'Boolean("1") must return true');

  assertSameValue(
    typeof !!("false"),
    "boolean",
    'The value of `typeof Boolean("false")` is expected to be "boolean"'
  );

  assertSameValue(!!("false"), true, 'Boolean("false") must return true');

  assertSameValue(
    typeof !!("true"),
    "boolean",
    'The value of `typeof Boolean("true")` is expected to be "boolean"'
  );

  assertSameValue(!!("true"), true, 'Boolean("true") must return true');
}
