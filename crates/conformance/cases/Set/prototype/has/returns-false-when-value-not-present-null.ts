// test262: test/built-ins/Set/prototype/has/returns-false-when-value-not-present-null.js

function main(): void {
  const s = new Set<number | null>();

  assertSameValue(s.has(null), false, "`s.has(null)` returns `false`");
}
