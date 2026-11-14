// test262: test/built-ins/Object/is/not-same-value-x-y-object.js
// Two distinct objects are never the same value, whatever their content. The
// `Object()` / `new Object(…)` wrapper variants have no counterpart (no
// constructors); plain literals and arrays carry the intent.

function main(): void {
  assertSameValue(Object.is({}, {}), false, "`Object.is({}, {})` returns `false`");
  assertSameValue(
    Object.is({ x: 0 }, { x: 0 }),
    false,
    "`Object.is({ x: 0 }, { x: 0 })` returns `false`",
  );
  const empty: number[] = [];
  const other: number[] = [];
  assertSameValue(Object.is(empty, other), false, "`Object.is([], [])` returns `false`");
}
