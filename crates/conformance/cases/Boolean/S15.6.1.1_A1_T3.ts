// test262: test/built-ins/Boolean/S15.6.1.1_A1_T3.js
// expect-error: unresolved identifier `Boolean`
//
// ToBoolean of strings (empty falsy, nonempty truthy) is truthy coercion,
// excluded by design — no `Boolean(x)` callable exists. The original's
// `typeof Boolean(...)` arms are dropped (typeof is a narrowing guard only).

function main(): void {
  assertSameValue(Boolean("0"), true, 'Boolean("0") must return true');
  assertSameValue(Boolean("-1"), true, 'Boolean("-1") must return true');
  assertSameValue(Boolean("1"), true, 'Boolean("1") must return true');
  assertSameValue(Boolean("false"), true, 'Boolean("false") must return true');
  assertSameValue(Boolean("true"), true, 'Boolean("true") must return true');
}
