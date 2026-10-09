// test262: test/built-ins/Set/prototype/forEach/returns-undefined.js

function main(): void {
  const s = new Set<number>([1]);

  assertSameValue(
    s.forEach((): void => {}),
    undefined,
    "`s.forEach(function() {})` returns `undefined`",
  );
}
