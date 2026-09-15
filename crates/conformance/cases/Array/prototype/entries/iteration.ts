// test262: test/built-ins/Array/prototype/entries/iteration.js
// Adapted: `value` is read through a runtime-checked `as
// IteratorYieldResult<...>` cast — the `done` field is typed boolean (not a
// literal), so it does not narrow the IteratorResult union. The exhausted
// result carries no value field (vs undefined in JS).

function main(): void {
  const array = ["a", "b", "c"];
  const iterator = array.entries();

  let result = iterator.next();
  assertSameValue(result.done, false, "First result `done` flag");
  let yielded = result as IteratorYieldResult<[number, string]>;
  assertSameValue(yielded.value[0], 0, "First result `value` (array key)");
  assertSameValue(yielded.value[1], "a", "First result `value` (array value)");
  assertSameValue(yielded.value.length, 2, "First result `value` (length)");

  result = iterator.next();
  assertSameValue(result.done, false, "Second result `done` flag");
  yielded = result as IteratorYieldResult<[number, string]>;
  assertSameValue(yielded.value[0], 1, "Second result `value` (array key)");
  assertSameValue(yielded.value[1], "b", "Second result `value` (array value)");
  assertSameValue(yielded.value.length, 2, "Second result `value` (length)");

  result = iterator.next();
  assertSameValue(result.done, false, "Third result `done` flag");
  yielded = result as IteratorYieldResult<[number, string]>;
  assertSameValue(yielded.value[0], 2, "Third result `value` (array key)");
  assertSameValue(yielded.value[1], "c", "Third result `value` (array value)");
  assertSameValue(yielded.value.length, 2, "Third result `value` (length)");

  result = iterator.next();
  assertSameValue(result.done, true, "Exhausted result `done` flag");
}
