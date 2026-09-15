// test262: test/built-ins/Object/is/same-value-x-y-null.js

function main(): void {
  assertSameValue(Object.is(null, null), true, "`Object.is(null, null)` returns `true`");
}
