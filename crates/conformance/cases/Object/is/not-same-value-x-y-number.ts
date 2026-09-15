// test262: test/built-ins/Object/is/not-same-value-x-y-number.js
// The one-argument `Object.is(0)` variant (comparison against `undefined`)
// is dropped — no `undefined`, and both parameters are required.

function main(): void {
  assertSameValue(Object.is(+0, -0), false, "`Object.is(+0, -0)` returns `false`");
  assertSameValue(Object.is(-0, +0), false, "`Object.is(-0, +0)` returns `false`");
  assertSameValue(
    Object.is(Infinity, -Infinity),
    false,
    "`Object.is(Infinity, -Infinity)` returns `false`",
  );
}
