// test262: test/built-ins/Object/entries/primitive-booleans.js

function main(): void {
  const trueResult = Object.entries(true);

  assertSameValue(Array.isArray(trueResult), true, "trueResult is an array");
  assertSameValue(trueResult.length, 0, "trueResult has 0 items");

  const falseResult = Object.entries(false);

  assertSameValue(Array.isArray(falseResult), true, "falseResult is an array");
  assertSameValue(falseResult.length, 0, "falseResult has 0 items");
}
