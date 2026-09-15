// test262: test/built-ins/Object/is/not-same-value-x-y-type.js
// `undefined` comparison lines are dropped (no `undefined`).

function main(): void {
  const a = {};

  assertSameValue(Object.is(a, true), false, "`Object.is(a, true)` returns `false`");
  assertSameValue(Object.is(a, ""), false, "`Object.is(a, '')` returns `false`");
  assertSameValue(Object.is(a, 0), false, "`Object.is(a, 0)` returns `false`");

  assertSameValue(Object.is(NaN, true), false, "`Object.is(NaN, true)` returns `false`");
  assertSameValue(Object.is(NaN, ""), false, "`Object.is(NaN, '')` returns `false`");
  assertSameValue(Object.is(NaN, a), false, "`Object.is(NaN, a)` returns `false`");
  assertSameValue(Object.is(NaN, null), false, "`Object.is(NaN, null)` returns `false`");

  assertSameValue(Object.is(true, 0), false, "`Object.is(true, 0)` returns `false`");
  assertSameValue(Object.is(true, a), false, "`Object.is(true, a)` returns `false`");
  assertSameValue(Object.is(true, null), false, "`Object.is(true, null)` returns `false`");
  assertSameValue(Object.is(true, NaN), false, "`Object.is(true, NaN)` returns `false`");
  assertSameValue(Object.is(true, ""), false, "`Object.is(true, '')` returns `false`");

  assertSameValue(Object.is(false, 0), false, "`Object.is(false, 0)` returns `false`");
  assertSameValue(Object.is(false, a), false, "`Object.is(false, a)` returns `false`");
  assertSameValue(Object.is(false, null), false, "`Object.is(false, null)` returns `false`");
  assertSameValue(Object.is(false, NaN), false, "`Object.is(false, NaN)` returns `false`");
  assertSameValue(Object.is(false, ""), false, "`Object.is(false, '')` returns `false`");

  assertSameValue(Object.is(0, true), false, "`Object.is(0, true)` returns `false`");
  assertSameValue(Object.is(0, a), false, "`Object.is(0, a)` returns `false`");
  assertSameValue(Object.is(0, null), false, "`Object.is(0, null)` returns `false`");
  assertSameValue(Object.is(0, NaN), false, "`Object.is(0, NaN)` returns `false`");
  assertSameValue(Object.is(0, ""), false, "`Object.is(0, '')` returns `false`");
}
