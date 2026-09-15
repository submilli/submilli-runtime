// test262: test/built-ins/Map/prototype/delete/does-not-break-iterators.js
// Adapted: an exhausted result carries no `value` field here (no undefined);
// presence is checked via `"value" in result`.

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

  if ("value" in n) {
    const entry = n.value;
    assertSameValue(entry[0], "c");
    assertSameValue(entry[1], 3);
  } else {
    assert(false, "second next() should yield the entry after the deleted one");
  }

  const last = e.next();
  assert(!("value" in last), "exhausted result carries no value");
  assertSameValue(last.done, true);
}
