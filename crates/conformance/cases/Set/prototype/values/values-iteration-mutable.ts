// test262: test/built-ins/Set/prototype/values/values-iteration-mutable.js
// Adapted: an exhausted result carries no `value` field here (no undefined);
// yield results are narrowed via `"value" in result` and copied to a local
// (calls invalidate narrowing).

function main(): void {
  const set = new Set<number>();
  set.add(1);
  set.add(2);

  const iterator = set.values();

  const r1 = iterator.next();
  assertSameValue(r1.done, false, "First result `done` flag");
  if ("value" in r1) {
    const v = r1.value;
    assertSameValue(v, 1, "First result `value`");
  } else {
    assert(false, "first result should yield a value");
  }

  set.add(3);

  const r2 = iterator.next();
  assertSameValue(r2.done, false, "Second result `done` flag");
  if ("value" in r2) {
    const v = r2.value;
    assertSameValue(v, 2, "Second result `value`");
  } else {
    assert(false, "second result should yield a value");
  }

  const r3 = iterator.next();
  assertSameValue(r3.done, false, "Third result `done` flag");
  if ("value" in r3) {
    const v = r3.value;
    assertSameValue(v, 3, "Third result `value`");
  } else {
    assert(false, "third result should yield a value");
  }

  const r4 = iterator.next();
  assert(!("value" in r4), "Exhausted result carries no value");
  assertSameValue(r4.done, true, "Exhausted result `done` flag");

  set.add(4);

  const r5 = iterator.next();
  assert(!("value" in r5), "Exhausted result carries no value (repeated request)");
  assertSameValue(r5.done, true, "Exhausted result `done` flag (repeated request)");
}
