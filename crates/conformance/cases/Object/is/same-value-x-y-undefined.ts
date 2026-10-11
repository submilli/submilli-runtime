// test262: test/built-ins/Object/is/same-value-x-y-undefined.js

function main(): void {
  assertSameValue(Object.is(undefined, undefined), true, "`Object.is(undefined, undefined)` returns `true`");
  assertSameValue(Object.is(undefined), true, "`Object.is(undefined)` returns `true`");
}
