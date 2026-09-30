// test262: test/built-ins/Error/isError/fake-errors.js
//
// No prototypes, `stack`, or Symbol.toStringTag here, so the fake error is
// the part that can still be faked: an object with the fields of an Error.
// Error-ness is nominal, not structural.

function main(): void {
  const fakeError = {
    name: 'Error',
    message: '',
  };

  assertSameValue(Error.isError(fakeError), false);
}
