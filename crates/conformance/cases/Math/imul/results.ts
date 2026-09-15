// test262: test/built-ins/Math/imul/results.js

function main(): void {
  assertSameValue(Math.imul(2, 4), 8, "(2, 4)");
  assertSameValue(Math.imul(-1, 8), -8, "(-1, 8)");
  assertSameValue(Math.imul(-2, -2), 4, "(-2, -2)");

  assertSameValue(Math.imul(-0, 7), 0);
  assertSameValue(Math.imul(7, -0), 0);

  assertSameValue(Math.imul(0.1, 7), 0);
  assertSameValue(Math.imul(7, 0.1), 0);
  assertSameValue(Math.imul(0.9, 7), 0);
  assertSameValue(Math.imul(7, 0.9), 0);
  assertSameValue(Math.imul(1.1, 7), 7);
  assertSameValue(Math.imul(7, 1.1), 7);
  assertSameValue(Math.imul(1.9, 7), 7);
  assertSameValue(Math.imul(7, 1.9), 7);

  assertSameValue(Math.imul(1073741824, 7), -1073741824);
  assertSameValue(Math.imul(7, 1073741824), -1073741824);
  assertSameValue(Math.imul(1073741824, 1073741824), 0);

  assertSameValue(Math.imul(-1073741824, 7), 1073741824);
  assertSameValue(Math.imul(7, -1073741824), 1073741824);
  assertSameValue(Math.imul(-1073741824, -1073741824), 0);

  assertSameValue(Math.imul(-2147483648, 7), -2147483648);
  assertSameValue(Math.imul(7, -2147483648), -2147483648);
  assertSameValue(Math.imul(-2147483648, -2147483648), 0);

  assertSameValue(Math.imul(2147483647, 7), 2147483641);
  assertSameValue(Math.imul(7, 2147483647), 2147483641);
  assertSameValue(Math.imul(2147483647, 2147483647), 1);

  assertSameValue(Math.imul(4294967295, 5), -5);
  assertSameValue(Math.imul(4294967294, 5), -10);
  assertSameValue(Math.imul(2147483648, 7), -2147483648);

  assertSameValue(Math.imul(65536, 65536), 0);
  assertSameValue(Math.imul(65535, 65536), -65536);
  assertSameValue(Math.imul(65536, 65535), -65536);
  assertSameValue(Math.imul(65535, 65535), -131071);
}
