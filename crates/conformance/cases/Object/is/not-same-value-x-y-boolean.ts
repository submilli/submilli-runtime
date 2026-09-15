// test262: test/built-ins/Object/is/not-same-value-x-y-boolean.js
// `undefined` comparison lines are dropped (no `undefined`); the rest port
// directly — mixed-type arguments flow through the `unknown` parameters.

function main(): void {
  const emptyObj = {};
  const emptyArr: number[] = [];

  assertSameValue(Object.is(true, false), false, "`Object.is(true, false)` returns `false`");
  assertSameValue(Object.is(false, true), false, "`Object.is(false, true)` returns `false`");
  assertSameValue(Object.is(true, 1), false, "`Object.is(true, 1)` returns `false`");
  assertSameValue(Object.is(false, 0), false, "`Object.is(false, 0)` returns `false`");
  assertSameValue(Object.is(true, emptyObj), false, "`Object.is(true, {})` returns `false`");
  assertSameValue(Object.is(true, null), false, "`Object.is(true, null)` returns `false`");
  assertSameValue(Object.is(false, null), false, "`Object.is(false, null)` returns `false`");
  assertSameValue(Object.is(true, NaN), false, "`Object.is(true, NaN)` returns `false`");
  assertSameValue(Object.is(false, NaN), false, "`Object.is(false, NaN)` returns `false`");
  assertSameValue(Object.is(true, ""), false, "`Object.is(true, '')` returns `false`");
  assertSameValue(Object.is(false, ""), false, "`Object.is(false, '')` returns `false`");
  assertSameValue(Object.is(true, emptyArr), false, "`Object.is(true, [])` returns `false`");
  assertSameValue(Object.is(false, emptyArr), false, "`Object.is(false, [])` returns `false`");
}
