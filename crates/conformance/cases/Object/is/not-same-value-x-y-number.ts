// test262: test/built-ins/Object/is/not-same-value-x-y-number.js
// Source: https://github.com/tc39/test262/blob/d53ebc8b1931d9f1541e43d59650776e4b766248/test/built-ins/Object/is/not-same-value-x-y-number.js
// The required second argument is explicit undefined, preserving the runtime
// meaning of the upstream omitted-argument calls. assert.sameValue uses the shim.

function main(): void {
  assertSameValue(Object.is(+0, -0), false, "`Object.is(+0, -0)` returns `false`");
  assertSameValue(Object.is(-0, +0), false, "`Object.is(-0, +0)` returns `false`");
  assertSameValue(Object.is(0, undefined), false, "`Object.is(0)` returns `false`");
  assertSameValue(Object.is(Infinity, -Infinity), false, "`Object.is(Infinity, -Infinity)` returns `false`");
}
