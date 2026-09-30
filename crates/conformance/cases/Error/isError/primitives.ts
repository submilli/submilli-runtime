// test262: test/built-ins/Error/isError/primitives.js
//
// `Error.isError()` with no argument and `Error.isError(undefined)` are
// dropped: the parameter is required and there is no `undefined`.

function main(): void {
  assertSameValue(Error.isError(null), false);
  assertSameValue(Error.isError(true), false);
  assertSameValue(Error.isError(false), false);
  assertSameValue(Error.isError(0), false);
  assertSameValue(Error.isError(-0), false);
  assertSameValue(Error.isError(NaN), false);
  assertSameValue(Error.isError(Infinity), false);
  assertSameValue(Error.isError(-Infinity), false);
  assertSameValue(Error.isError(42), false);
  assertSameValue(Error.isError(''), false);
  assertSameValue(Error.isError('foo'), false);
}
