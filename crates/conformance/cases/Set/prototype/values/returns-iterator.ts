// test262: test/built-ins/Set/prototype/values/returns-iterator.js
// Yield results are narrowed before reading their value.

function main(): void {
  const set = new Set<number>();
  set.add(1);
  set.add(2);
  set.add(3);

  const iterator = set.values();

  const r1 = iterator.next();
  assertSameValue(r1.done, false, "First result `done` flag");
  if (r1.value !== undefined) {
    const v = r1.value;
    assertSameValue(v, 1, "First result `value`");
  } else {
    assert(false, "first result should yield a value");
  }

  const r2 = iterator.next();
  assertSameValue(r2.done, false, "Second result `done` flag");
  if (r2.value !== undefined) {
    const v = r2.value;
    assertSameValue(v, 2, "Second result `value`");
  } else {
    assert(false, "second result should yield a value");
  }

  const r3 = iterator.next();
  assertSameValue(r3.done, false, "Third result `done` flag");
  if (r3.value !== undefined) {
    const v = r3.value;
    assertSameValue(v, 3, "Third result `value`");
  } else {
    assert(false, "third result should yield a value");
  }

  const r4 = iterator.next();
  assertSameValue(r4.value, undefined, "Exhausted result has undefined value");
  assertSameValue(r4.done, true, "Exhausted result `done` flag");

  const r5 = iterator.next();
  assertSameValue(r5.value, undefined, "Exhausted result has undefined value (repeated request)");
  assertSameValue(r5.done, true, "Exhausted result `done` flag (repeated request)");
}
