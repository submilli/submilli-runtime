// test262: test/built-ins/JSON/stringify/value-primitive-top-level.js
// The original's `JSON.stringify(undefined)` is dropped: there is no
// `undefined` in this language.

function main(): void {
  assertSameValue(JSON.stringify(null), "null");
  assertSameValue(JSON.stringify(true), "true");
  assertSameValue(JSON.stringify(false), "false");
  assertSameValue(JSON.stringify("str"), "\"str\"");
  assertSameValue(JSON.stringify(123), "123");
}
