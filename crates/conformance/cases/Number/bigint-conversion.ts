// test262: test/built-ins/Number/bigint-conversion.js
//
// The new Number(0n) boxing arm is dropped (no boxing).

function main(): void {
  assertSameValue(Number(0n), 0);

  assertSameValue(Number(2n ** 53n), 9007199254740992);
  assertSameValue(Number(2n ** 53n + 1n), 9007199254740992);
  assertSameValue(Number(2n ** 53n + 2n), 9007199254740994);
  assertSameValue(Number(2n ** 53n + 3n), 9007199254740996);
  assertSameValue(Number(2n ** 53n + 4n), 9007199254740996);

  assertSameValue(Number(-(2n ** 53n)), -9007199254740992);
  assertSameValue(Number(-(2n ** 53n + 1n)), -9007199254740992);
  assertSameValue(Number(-(2n ** 53n + 2n)), -9007199254740994);
  assertSameValue(Number(-(2n ** 53n + 3n)), -9007199254740996);
  assertSameValue(Number(-(2n ** 53n + 4n)), -9007199254740996);
}
