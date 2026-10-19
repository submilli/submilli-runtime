// test262: test/built-ins/Boolean/S15.6.1.1_A1_T3.js
// Adapted: Boolean(value) is spelled !!value; Boolean is not callable here.
// Adapted: named string operands avoid always-truthy literal diagnostics.

function main(): void {
  const zero: string = "0";
  const negativeOne: string = "-1";
  const one: string = "1";
  const falseString: string = "false";
  const trueString: string = "true";

  assertSameValue(typeof !!zero, "boolean", 'The value of `typeof Boolean("0")` is expected to be "boolean"');
  assertSameValue(!!zero, true, 'Boolean("0") must return true');
  assertSameValue(typeof !!negativeOne, "boolean", 'The value of `typeof Boolean("-1")` is expected to be "boolean"');
  assertSameValue(!!negativeOne, true, 'Boolean("-1") must return true');
  assertSameValue(typeof !!one, "boolean", 'The value of `typeof Boolean("1")` is expected to be "boolean"');
  assertSameValue(!!one, true, 'Boolean("1") must return true');

  assertSameValue(
    typeof !!falseString,
    "boolean",
    'The value of `typeof Boolean("false")` is expected to be "boolean"'
  );

  assertSameValue(!!falseString, true, 'Boolean("false") must return true');

  assertSameValue(
    typeof !!trueString,
    "boolean",
    'The value of `typeof Boolean("true")` is expected to be "boolean"'
  );

  assertSameValue(!!trueString, true, 'Boolean("true") must return true');
}
