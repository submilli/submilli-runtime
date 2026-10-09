// test262: test/built-ins/Array/prototype/keys/iteration.js
// Adapted: `value` is read through a runtime-checked `as
// IteratorYieldResult<number>` cast — the `done` field is typed boolean (not
// a literal), so it does not narrow the IteratorResult union.

function main(): void {
  const array = ["a", "b", "c"];
  const iterator = array.keys();

  let result = iterator.next();
  assertSameValue((result as IteratorYieldResult<number>).value, 0, "First result `value`");
  assertSameValue(result.done, false, "First result `done` flag");

  result = iterator.next();
  assertSameValue((result as IteratorYieldResult<number>).value, 1, "Second result `value`");
  assertSameValue(result.done, false, "Second result `done` flag");

  result = iterator.next();
  assertSameValue((result as IteratorYieldResult<number>).value, 2, "Third result `value`");
  assertSameValue(result.done, false, "Third result `done` flag");

  result = iterator.next();
  assertSameValue(result.done, true, "Exhausted result `done` flag");
  assertSameValue(result.value, undefined, "Exhausted result `value`");
}
