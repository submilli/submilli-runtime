// test262: test/built-ins/NativeErrors/message_property_native_error.js
//
// The harness's nativeErrors list is narrowed to the built-in subclasses
// that exist here (RangeError, SyntaxError, TypeError — spec.md §1.8) and
// unrolled, since classes are not values to iterate over. The enumerable and
// configurable checks are dropped (blanket rule); the value and write checks
// are kept.

function main(): void {
  const message = "my-message";

  const rangeError = new RangeError(message);
  assertSameValue(rangeError.message, message, "RangeError message");
  rangeError.message = "rewritten";
  assertSameValue(rangeError.message, "rewritten", "RangeError message is writable");

  const syntaxError = new SyntaxError(message);
  assertSameValue(syntaxError.message, message, "SyntaxError message");
  syntaxError.message = "rewritten";
  assertSameValue(syntaxError.message, "rewritten", "SyntaxError message is writable");

  const typeError = new TypeError(message);
  assertSameValue(typeError.message, message, "TypeError message");
  typeError.message = "rewritten";
  assertSameValue(typeError.message, "rewritten", "TypeError message is writable");
}
