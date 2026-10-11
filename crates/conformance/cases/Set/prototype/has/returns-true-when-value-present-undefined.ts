// test262: test/built-ins/Set/prototype/has/returns-true-when-value-present-undefined.js

function main(): void {
  const s = new Set<undefined>();

  s.add(undefined);

  assertSameValue(s.has(undefined), true, "`s.has(undefined)` returns `true`");
}
