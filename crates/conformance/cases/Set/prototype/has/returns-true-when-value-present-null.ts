// test262: test/built-ins/Set/prototype/has/returns-true-when-value-present-null.js
// expect-fail: null Set elements trap at runtime (the equals/hash vtable dispatch on a null ref) — the standard stores and finds null

function main(): void {
  const s = new Set<number | null>();

  s.add(null);

  assertSameValue(s.has(null), true, "`s.has(null)` returns `true`");
}
