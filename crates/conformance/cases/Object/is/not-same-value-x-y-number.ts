// test262: test/built-ins/Object/is/not-same-value-x-y-number.js

function main(): void {
  assertSameValue(Object.is(+0, -0), false, "`Object.is(+0, -0)` returns `false`");
  assertSameValue(Object.is(-0, +0), false, "`Object.is(-0, +0)` returns `false`");
  assertSameValue(Object.is(0), false, "`Object.is(0)` returns `false`");
  assertSameValue(Object.is(Infinity, -Infinity), false, "`Object.is(Infinity, -Infinity)` returns `false`");
}
