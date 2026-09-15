// test262: test/built-ins/Array/prototype/values/iteration.js
// Adapted: `value` is read through a runtime-checked `as
// IteratorYieldResult<string>` cast — the `done` field is typed boolean (not
// a literal), so it does not narrow the IteratorResult union. The exhausted
// result carries no value field (vs undefined in JS).

function main(): void {
  const array = ["a", "b", "c"];
  const iterator = array.values();

  let result = iterator.next();
  assertSameValue((result as IteratorYieldResult<string>).value, "a", "First result `value`");
  assertSameValue(result.done, false, "First result `done` flag");

  result = iterator.next();
  assertSameValue((result as IteratorYieldResult<string>).value, "b", "Second result `value`");
  assertSameValue(result.done, false, "Second result `done` flag");

  result = iterator.next();
  assertSameValue((result as IteratorYieldResult<string>).value, "c", "Third result `value`");
  assertSameValue(result.done, false, "Third result `done` flag");

  result = iterator.next();
  assertSameValue(result.done, true, "Exhausted result `done` flag");
}
