// test262: test/built-ins/Map/prototype/delete/returns-false.js

function main(): void {
  const m = new Map<string, number>([
    ["a", 1],
    ["b", 2],
  ]);

  assertSameValue(m.delete("not-in-the-map"), false);

  m.delete("a");
  assertSameValue(m.delete("a"), false);
}
