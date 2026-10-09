// test262: test/built-ins/Set/prototype/clear/returns-undefined.js

function main(): void {
  const s = new Set<number>();

  assertSameValue(s.clear(), undefined, "`s.clear()` returns `undefined`");
}
