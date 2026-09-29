// test262: test/built-ins/Error/isError/errors.js
//
// Only Error and its built-in subclasses RangeError, SyntaxError, and
// TypeError exist (spec.md §1.8); the EvalError, ReferenceError, URIError,
// AggregateError, and SuppressedError arms are dropped. Constructors require
// a message, so `new X()` becomes `new X("")`.

function main(): void {
  assertSameValue(Error.isError(new Error("")), true);
  assertSameValue(Error.isError(new RangeError("")), true);
  assertSameValue(Error.isError(new SyntaxError("")), true);
  assertSameValue(Error.isError(new TypeError("")), true);
}
