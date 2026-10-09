// test262: test/built-ins/Set/prototype/has/returns-false-when-value-not-present-undefined.js

function main(): void {
  const s = new Set<number | undefined>();

  assertSameValue(s.has(undefined), false, "`s.has(undefined)` returns `false`");
}
