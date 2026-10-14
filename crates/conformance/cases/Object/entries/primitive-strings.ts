// test262: test/built-ins/Object/entries/primitive-strings.js

function main(): void {
  const result = Object.entries("abc");

  assertSameValue(Array.isArray(result), true, "result is an array");
  assertSameValue(result.length, 3, "result has 3 items");

  assertSameValue(result[0][0], "0", "first entry has key \"0\"");
  assertSameValue(result[0][1] as string, "a", "first entry has value \"a\"");
  assertSameValue(result[1][0], "1", "second entry has key \"1\"");
  assertSameValue(result[1][1] as string, "b", "second entry has value \"b\"");
  assertSameValue(result[2][0], "2", "third entry has key \"2\"");
  assertSameValue(result[2][1] as string, "c", "third entry has value \"c\"");
}
