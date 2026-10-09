// test262: test/built-ins/Map/prototype/delete/does-not-break-iterators.js
// Yield results are narrowed before reading their value.

function main(): void {
  const m = new Map<string, number>([
    ["a", 1],
    ["b", 2],
    ["c", 3],
  ]);
  const e = m.entries();

  e.next();
  m.delete("b");

  const n = e.next();

  if (n.value !== undefined) {
    const entry = n.value;
    assertSameValue(entry[0], "c");
    assertSameValue(entry[1], 3);
  } else {
    assert(false, "second next() should yield the entry after the deleted one");
  }

  const last = e.next();
  assertSameValue(last.value, undefined, "exhausted result has undefined value");
  assertSameValue(last.done, true);
}
