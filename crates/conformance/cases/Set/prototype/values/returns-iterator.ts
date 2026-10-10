// test262: test/built-ins/Set/prototype/values/returns-iterator.js

function main(): void {
  const set = new Set<number>();
  set.add(1);
  set.add(2);
  set.add(3);

  const iterator = set.values();

  let result = iterator.next();
  assertSameValue(result.value, 1, "First result `value`");
  assertSameValue(result.done, false, "First result `done` flag");

  result = iterator.next();
  assertSameValue(result.value, 2, "Second result `value`");
  assertSameValue(result.done, false, "Second result `done` flag");

  result = iterator.next();
  assertSameValue(result.value, 3, "Third result `value`");
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
