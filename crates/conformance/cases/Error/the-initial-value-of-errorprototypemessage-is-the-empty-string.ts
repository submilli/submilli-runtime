// test262: test/built-ins/Error/the-initial-value-of-errorprototypemessage-is-the-empty-string.js
// expect-error: `Error` is a class, not a value
//
// Calling `Error(...)` without `new` is not supported: classes are only
// constructible, so the first assertion is a compile error. The
// hasOwnProperty and Error.prototype.message arms are dropped (no prototypes);
// the `new Error('a')` arm alone is covered by message_property.ts.

function main(): void {
  assertSameValue(Error('a').message, "a", 'The value of err1.message is "a"');
  assertSameValue(new Error('a').message, "a", 'The value of err1.message is "a"');
}
