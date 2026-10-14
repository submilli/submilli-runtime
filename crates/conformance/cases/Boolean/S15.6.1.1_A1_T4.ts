// test262: test/built-ins/Boolean/S15.6.1.1_A1_T4.js
// Adapted: Boolean(value) uses !!value; the Boolean constructor is unsupported.
// Adapted: void-completion calls use `void call()` before coercion.
// Adapted: the hoisted uninitialized var is explicitly initialized to undefined.
// Adapted: named operands avoid always-falsy undefined, void and null diagnostics.

function main(): void {
  const x: undefined = undefined;
  const undefinedValue: undefined = undefined;
  const voidZero: undefined = void 0;
  const voidCompletion: undefined = void ((): void => {})();
  const nullValue: null = null;

  assertSameValue(
    typeof !!undefinedValue,
    "boolean",
    'The value of `typeof Boolean(undefined)` is expected to be "boolean"'
  );

  assertSameValue(!!undefinedValue, false, 'Boolean(undefined) must return false');

  assertSameValue(
    typeof !!voidZero,
    "boolean",
    'The value of `typeof Boolean(void 0)` is expected to be "boolean"'
  );

  assertSameValue(!!voidZero, false, 'Boolean(void 0) must return false');

  assertSameValue(
    typeof !!voidCompletion,
    "boolean",
    'The value of `typeof Boolean(((): void => {})())` is expected to be "boolean"'
  );

  assertSameValue(!!voidCompletion, false, 'Boolean(((): void => {})()) must return false');
  assertSameValue(typeof !!nullValue, "boolean", 'The value of `typeof Boolean(null)` is expected to be "boolean"');
  assertSameValue(!!nullValue, false, 'Boolean(null) must return false');
  assertSameValue(typeof !!(x), "boolean", 'The value of `typeof Boolean(x)` is expected to be "boolean"');
  assertSameValue(!!(x), false, 'Boolean() must return false');
}
