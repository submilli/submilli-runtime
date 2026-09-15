// test262: test/built-ins/Set/prototype/has/returns-false-when-value-not-present-nan.js

function main(): void {
  const s = new Set<number>();

  assertSameValue(s.has(NaN), false, "`s.has(NaN)` returns `false`");
}
