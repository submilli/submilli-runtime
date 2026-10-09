// test262: test/built-ins/Array/prototype/values/iteration.js

function main(): void {
  const array = ["a", "b", "c"];
  const iterator = array.values();

  let result = iterator.next();
  assertSameValue(result.value, "a", "First result `value`");
  assertSameValue(result.done, false, "First result `done` flag");

  result = iterator.next();
  assertSameValue(result.value, "b", "Second result `value`");
  assertSameValue(result.done, false, "Second result `done` flag");

  result = iterator.next();
  assertSameValue(result.value, "c", "Third result `value`");
  assertSameValue(result.done, false, "Third result `done` flag");

  result = iterator.next();
  assertSameValue(result.value, undefined, "Exhausted result `value`");
  assertSameValue(result.done, true, "Exhausted result `done` flag");
}
