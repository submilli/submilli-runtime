// test262: test/built-ins/JSON/stringify/value-primitive-top-level.js

function main(): void {
  assertSameValue(JSON.stringify(null), "null");
  assertSameValue(JSON.stringify(true), "true");
  assertSameValue(JSON.stringify(false), "false");
  assertSameValue(JSON.stringify("str"), "\"str\"");
  assertSameValue(JSON.stringify(123), "123");
  // A check sees the missing JSON text as undefined. A use typed `string`
  // throws MissingValueError; `Object.is` takes `unknown`, which does not
  // accept that missing result (SUB-2190).
  assert(JSON.stringify(undefined) === undefined);
}
