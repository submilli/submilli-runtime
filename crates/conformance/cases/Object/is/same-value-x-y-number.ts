// test262: test/built-ins/Object/is/same-value-x-y-number.js

function main(): void {
  assertSameValue(Object.is(NaN, NaN), true, "`Object.is(NaN, NaN)` returns `true`");
  assertSameValue(Object.is(-0, -0), true, "`Object.is(-0, -0)` returns `true`");
  assertSameValue(Object.is(+0, +0), true, "`Object.is(+0, +0)` returns `true`");
  assertSameValue(Object.is(0, 0), true, "`Object.is(0, 0)` returns `true`");
}
