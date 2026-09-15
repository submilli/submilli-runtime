// test262: test/built-ins/Object/is/same-value-x-y-boolean.js

function main(): void {
  assertSameValue(Object.is(true, true), true, "`Object.is(true, true)` returns `true`");
  assertSameValue(Object.is(false, false), true, "`Object.is(false, false)` returns `true`");
}
