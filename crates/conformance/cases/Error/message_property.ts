// test262: test/built-ins/Error/message_property.js
//
// verifyEqualTo is a value check plus a write; the descriptor half
// (enumerable/configurable) is dropped by the blanket rule. `message` is a
// mutable field per spec.md §1.8, so the write is kept.

function main(): void {
  const message = "my-message";
  const error = new Error(message);

  assertSameValue(error.message, message, "error.message is the constructor argument");
  error.message = "rewritten";
  assertSameValue(error.message, "rewritten", "error.message is writable");
}
