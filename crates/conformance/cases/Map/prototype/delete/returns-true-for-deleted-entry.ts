// test262: test/built-ins/Map/prototype/delete/returns-true-for-deleted-entry.js

function main(): void {
  const m = new Map<string, number>([
    ["a", 1],
    ["b", 2],
  ]);

  const result = m.delete("a");

  assert(result);
  assertSameValue(m.size, 1);
}
