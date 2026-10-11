// test262: test/built-ins/Boolean/S15.6.1.1_A1_T4.js
// Adapted: Boolean(value) uses !!value; the Boolean constructor is unsupported.
// Adapted: void-completion calls use `void call()` before coercion.
// Adapted: the hoisted uninitialized var is explicitly initialized to undefined.

function main(): void {
  const x: undefined = undefined;
  assertSameValue(
    typeof !!(undefined),
    "boolean",
    'The value of `typeof Boolean(undefined)` is expected to be "boolean"'
  );

  assertSameValue(!!(undefined), false, 'Boolean(undefined) must return false');

  assertSameValue(
    typeof !!(void 0),
    "boolean",
    'The value of `typeof Boolean(void 0)` is expected to be "boolean"'
  );

  assertSameValue(!!(void 0), false, 'Boolean(void 0) must return false');

  assertSameValue(
    typeof !!(void ((): void => {})()),
    "boolean",
    'The value of `typeof Boolean(((): void => {})())` is expected to be "boolean"'
  );

  assertSameValue(!!(void ((): void => {})()), false, 'Boolean(((): void => {})()) must return false');
  assertSameValue(typeof !!(null), "boolean", 'The value of `typeof Boolean(null)` is expected to be "boolean"');
  assertSameValue(!!(null), false, 'Boolean(null) must return false');
  assertSameValue(typeof !!(x), "boolean", 'The value of `typeof Boolean(x)` is expected to be "boolean"');
  assertSameValue(!!(x), false, 'Boolean() must return false');
}
