// test262: test/built-ins/Object/is/same-value-x-y-object.js
// Self-comparison holds under our structural Object.is as it does under JS
// identity. The `Object(0)` / `new Object('')` / `Array()` wrapper variants
// have no counterpart (no constructors); plain literals carry the intent.

function main(): void {
  const a = {};
  const b = { x: 0 };
  const d: number[] = [];

  assertSameValue(Object.is(a, a), true, "`Object.is(a, a)` returns `true`");
  assertSameValue(Object.is(b, b), true, "`Object.is(b, b)` returns `true`");
  assertSameValue(Object.is(d, d), true, "`Object.is(d, d)` returns `true`");
}
