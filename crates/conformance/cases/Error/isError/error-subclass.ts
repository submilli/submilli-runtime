// test262: test/built-ins/Error/isError/error-subclass.js
//
// Subclasses of the built-in error classes that exist here (spec.md §1.8);
// the EvalError, ReferenceError, URIError, AggregateError, and
// SuppressedError arms are dropped. The inherited constructor requires a
// message, so `new X()` becomes `new X("")`.

class MyError extends Error {}
class MyRangeError extends RangeError {}
class MySyntaxError extends SyntaxError {}
class MyTypeError extends TypeError {}

function main(): void {
  assertSameValue(Error.isError(new MyError("")), true);
  assertSameValue(Error.isError(new MyRangeError("")), true);
  assertSameValue(Error.isError(new MySyntaxError("")), true);
  assertSameValue(Error.isError(new MyTypeError("")), true);
}
