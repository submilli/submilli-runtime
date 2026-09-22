// test262: test/built-ins/Set/prototype/has/returns-true-when-value-present-null.js

function main(): void {
  const s = new Set<number | null>();

  s.add(null);

  assertSameValue(s.has(null), true, "`s.has(null)` returns `true`");
}
