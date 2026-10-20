// test262: test/built-ins/Array/prototype/concat/S15.4.4.4_A1_T2.js
// Source: https://github.com/tc39/test262/blob/d53ebc8b1931d9f1541e43d59650776e4b766248/test/built-ins/Array/prototype/concat/S15.4.4.4_A1_T2.js
// unknown[] preserves the mixed arguments. An object literal with a discriminator
// replaces new Object(); Array.isArray replaces prototype-based brand reflection.
// The SameValue shim compares objects structurally, not by reference identity.

function main(): void {
  const x: unknown[] = [0];
  const y = { marker: "concat object" };
  const z = [1, 2];
  const arr = x.concat(y, z, -1, true, "NaN");

  assertSameValue(Array.isArray(arr), true, "Array.isArray(arr) must return true");
  assertSameValue(arr[0], 0, 'The value of arr[0] is expected to be 0');
  assertSameValue(arr[1], y, 'The value of arr[1] is expected to equal the value of y');
  assertSameValue(arr[2], 1, 'The value of arr[2] is expected to be 1');
  assertSameValue(arr[3], 2, 'The value of arr[3] is expected to be 2');
  assertSameValue(arr[4], -1, 'The value of arr[4] is expected to be -1');
  assertSameValue(arr[5], true, 'The value of arr[5] is expected to be true');
  assertSameValue(arr[6], "NaN", 'The value of arr[6] is expected to be "NaN"');
  assertSameValue(arr.length, 7, 'The value of arr.length is expected to be 7');
}
