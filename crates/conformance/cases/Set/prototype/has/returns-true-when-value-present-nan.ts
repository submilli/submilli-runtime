// test262: test/built-ins/Set/prototype/has/returns-true-when-value-present-nan.js

function main(): void {
  const s = new Set<number>();

  s.add(NaN);

  assertSameValue(s.has(NaN), true, "`s.has(NaN)` returns `true`");
}
