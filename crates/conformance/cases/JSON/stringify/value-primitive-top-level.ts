// test262: test/built-ins/JSON/stringify/value-primitive-top-level.js

function main(): void {
  assertSameValue(JSON.stringify(null), "null");
  assertSameValue(JSON.stringify(true), "true");
  assertSameValue(JSON.stringify(false), "false");
  assertSameValue(JSON.stringify("str"), "\"str\"");
  assertSameValue(JSON.stringify(123), "123");
  assertSameValue(JSON.stringify(undefined), undefined);
}
