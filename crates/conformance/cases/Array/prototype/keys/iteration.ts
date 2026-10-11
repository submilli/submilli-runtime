// test262: test/built-ins/Array/prototype/keys/iteration.js

function main(): void {
  const array = ["a", "b", "c"];
  const iterator = array.keys();

  let result = iterator.next();
  assertSameValue(result.value, 0, "First result `value`");
  assertSameValue(result.done, false, "First result `done` flag");

  result = iterator.next();
  assertSameValue(result.value, 1, "Second result `value`");
  assertSameValue(result.done, false, "Second result `done` flag");

  result = iterator.next();
  assertSameValue(result.value, 2, "Third result `value`");
  assertSameValue(result.done, false, "Third result `done` flag");

  result = iterator.next();
  assertSameValue(result.value, undefined, "Exhausted result `value`");
  assertSameValue(result.done, true, "Exhausted result `done` flag");
}
