// test262: test/built-ins/Object/values/primitive-strings.js

function main(): void {
  const result = Object.values("abc");

  assertSameValue(Array.isArray(result), true, "result is an array");
  assertSameValue(result.length, 3, "result has 3 items");

  assertSameValue(result[0] as string, "a", "first value is \"a\"");
  assertSameValue(result[1] as string, "b", "second value is \"b\"");
  assertSameValue(result[2] as string, "c", "third value is \"c\"");
}
