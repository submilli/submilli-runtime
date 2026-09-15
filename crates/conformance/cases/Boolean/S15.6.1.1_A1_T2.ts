// test262: test/built-ins/Boolean/S15.6.1.1_A1_T2.js
// expect-error: unresolved identifier `Boolean`
//
// ToBoolean of numbers (0/NaN falsy, the rest truthy) is truthy coercion,
// excluded by design — no `Boolean(x)` callable exists. The original's
// `typeof Boolean(...)` arms are dropped (typeof is a narrowing guard only).

function main(): void {
  assertSameValue(Boolean(0), false, 'Boolean(0) must return false');
  assertSameValue(Boolean(-1), true, 'Boolean(-1) must return true');
  assertSameValue(Boolean(-Infinity), true, 'Boolean(-Infinity) must return true');
  assertSameValue(Boolean(NaN), false, 'Boolean(NaN) must return false');
}
