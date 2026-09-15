// test262: test/built-ins/JSON/stringify/value-number-non-finite.js

function main(): void {
  assertSameValue(JSON.stringify(Infinity), "null");
  assertSameValue(JSON.stringify({ key: -Infinity }), "{\"key\":null}");
  assertSameValue(JSON.stringify([NaN]), "[null]");
}
