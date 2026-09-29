// test262: test/built-ins/Error/isError/bigints.js

function main(): void {
  assertSameValue(Error.isError(0n), false);
  assertSameValue(Error.isError(42n), false);
  assertSameValue(Error.isError(BigInt(0)), false);
  assertSameValue(Error.isError(BigInt(42)), false);
}
