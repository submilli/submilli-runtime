// test262: test/built-ins/Object/is/same-value-x-y-undefined.js
// Source: https://github.com/tc39/test262/blob/d53ebc8b1931d9f1541e43d59650776e4b766248/test/built-ins/Object/is/same-value-x-y-undefined.js
// The required second argument is explicit undefined, preserving the runtime
// meaning of the upstream omitted-argument calls. assert.sameValue uses the shim.

function main(): void {
  assertSameValue(Object.is(undefined, undefined), true, "`Object.is(undefined, undefined)` returns `true`");
  assertSameValue(Object.is(undefined, undefined), true, "`Object.is(undefined)` returns `true`");
}
