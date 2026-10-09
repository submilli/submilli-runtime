// test262: test/built-ins/Object/is/not-same-value-x-y-boolean.js

function main(): void {
  const emptyArray: number[] = [];
  assertSameValue(Object.is(true, false), false, "`Object.is(true, false)` returns `false`");
  assertSameValue(Object.is(false, true), false, "`Object.is(false, true)` returns `false`");
  assertSameValue(Object.is(true, 1), false, "`Object.is(true, 1)` returns `false`");
  assertSameValue(Object.is(false, 0), false, "`Object.is(false, 0)` returns `false`");
  assertSameValue(Object.is(true, {}), false, "`Object.is(true, {})` returns `false`");
  assertSameValue(Object.is(true, undefined), false, "`Object.is(true, undefined)` returns `false`");
  assertSameValue(Object.is(false, undefined), false, "`Object.is(false, undefined)` returns `false`");
  assertSameValue(Object.is(true, null), false, "`Object.is(true, null)` returns `false`");
  assertSameValue(Object.is(false, null), false, "`Object.is(false, null)` returns `false`");
  assertSameValue(Object.is(true, NaN), false, "`Object.is(true, NaN)` returns `false`");
  assertSameValue(Object.is(false, NaN), false, "`Object.is(false, NaN)` returns `false`");
  assertSameValue(Object.is(true, ''), false, "`Object.is(true, '')` returns `false`");
  assertSameValue(Object.is(false, ''), false, "`Object.is(false, '')` returns `false`");
  assertSameValue(Object.is(true, emptyArray), false, "`Object.is(true, emptyArray)` returns `false`");
  assertSameValue(Object.is(false, emptyArray), false, "`Object.is(false, emptyArray)` returns `false`");
}
