// test262: test/built-ins/Object/is/not-same-value-x-y-string.js

function main(): void {
  assertSameValue(Object.is('', true), false, "`Object.is('', true)` returns `false`");
  assertSameValue(Object.is('', 0), false, "`Object.is('', 0)` returns `false`");
  assertSameValue(Object.is('', {}), false, "`Object.is('', {})` returns `false`");
  assertSameValue(
    Object.is('', undefined),
    false,
    "`Object.is('', undefined)` returns `false`"
  );
  assertSameValue(Object.is('', null), false, "`Object.is('', null)` returns `false`");
  assertSameValue(Object.is('', NaN), false, "`Object.is('', NaN)` returns `false`");
}
