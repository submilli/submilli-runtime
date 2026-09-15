// test262: test/built-ins/Map/prototype/entries/returns-iterator.js
// Adapted: an exhausted result carries no `value` field here (no undefined);
// yield results are narrowed via `"value" in result` and copied to a local
// (calls invalidate narrowing).

function main(): void {
  const map = new Map<string, number>();
  map.set("a", 1);
  map.set("b", 2);
  map.set("c", 3);

  const iterator = map.entries();

  const r1 = iterator.next();
  assertSameValue(r1.done, false, "First result `done` flag");
  if ("value" in r1) {
    const entry = r1.value;
    assertSameValue(entry[0], "a", "First result `value` (key)");
    assertSameValue(entry[1], 1, "First result `value` (value)");
    assertSameValue(entry.length, 2, "First result `value` (length)");
  } else {
    assert(false, "first result should yield a value");
  }

  const r2 = iterator.next();
  assertSameValue(r2.done, false, "Second result `done` flag");
  if ("value" in r2) {
    const entry = r2.value;
    assertSameValue(entry[0], "b", "Second result `value` (key)");
    assertSameValue(entry[1], 2, "Second result `value` (value)");
    assertSameValue(entry.length, 2, "Second result `value` (length)");
  } else {
    assert(false, "second result should yield a value");
  }

  const r3 = iterator.next();
  assertSameValue(r3.done, false, "Third result `done` flag");
  if ("value" in r3) {
    const entry = r3.value;
    assertSameValue(entry[0], "c", "Third result `value` (key)");
    assertSameValue(entry[1], 3, "Third result `value` (value)");
    assertSameValue(entry.length, 2, "Third result `value` (length)");
  } else {
    assert(false, "third result should yield a value");
  }

  const r4 = iterator.next();
  assert(!("value" in r4), "Exhausted result carries no value");
  assertSameValue(r4.done, true, "Exhausted result `done` flag");

  const r5 = iterator.next();
  assert(!("value" in r5), "Exhausted result carries no value (repeated request)");
  assertSameValue(r5.done, true, "Exhausted result `done` flag (repeated request)");
}
