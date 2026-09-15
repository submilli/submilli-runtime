// test262: test/built-ins/Boolean/S9.2_A3_T1.js
// expect-error: unresolved identifier `Boolean`
//
// There is no `Boolean(x)` callable: ToBoolean is truthy coercion, which the
// language excludes (conditions are strictly boolean). The prelude declares
// only `Number`/`String`/`BigInt` call signatures.

function main(): void {
  assertSameValue(Boolean(true), true, 'Boolean(true) must return true');
  assertSameValue(Boolean(false), false, 'Boolean(false) must return false');
}
