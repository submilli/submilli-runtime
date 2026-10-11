// test262: test/built-ins/Map/prototype/entries/returns-iterator.js
// Adapted: `result.value[i]` is read through an `as
// IteratorYieldResult<[string, number]>` cast — indexing needs the yield
// variant, and `done` is typed boolean (not a literal), so the IteratorResult
// union is not narrowed by the type alone.

function main(): void {
  const map = new Map<string, number>();
  map.set("a", 1);
  map.set("b", 2);
  map.set("c", 3);

  const iterator = map.entries();

  let result = iterator.next();
  assertSameValue((result as IteratorYieldResult<[string, number]>).value[0], "a", "First result `value` (\"key\")");
  assertSameValue((result as IteratorYieldResult<[string, number]>).value[1], 1, "First result `value` (\"value\")");
  assertSameValue((result as IteratorYieldResult<[string, number]>).value.length, 2, "First result `value` (length)");
  assertSameValue(result.done, false, "First result `done` flag");

  result = iterator.next();
  assertSameValue((result as IteratorYieldResult<[string, number]>).value[0], "b", "Second result `value` (\"key\")");
  assertSameValue((result as IteratorYieldResult<[string, number]>).value[1], 2, "Second result `value` (\"value\")");
  assertSameValue((result as IteratorYieldResult<[string, number]>).value.length, 2, "Second result `value` (length)");
  assertSameValue(result.done, false, "Second result `done` flag");

  result = iterator.next();
  assertSameValue((result as IteratorYieldResult<[string, number]>).value[0], "c", "Third result `value` (\"key\")");
  assertSameValue((result as IteratorYieldResult<[string, number]>).value[1], 3, "Third result `value` (\"value\")");
  assertSameValue((result as IteratorYieldResult<[string, number]>).value.length, 2, "Third result `value` (length)");
  assertSameValue(result.done, false, "Third result `done` flag");

  result = iterator.next();
  assertSameValue(result.value, undefined, "Exhausted result `value`");
  assertSameValue(result.done, true, "Exhausted result `done` flag");

  result = iterator.next();
  assertSameValue(
    result.value, undefined, "Exhausted result `value` (repeated request)",
  );
  assertSameValue(
    result.done, true, "Exhausted result `done` flag (repeated request)",
  );
}
