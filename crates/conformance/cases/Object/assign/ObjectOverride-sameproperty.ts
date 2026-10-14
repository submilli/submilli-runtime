// test262: test/built-ins/Object/assign/ObjectOverride-sameproperty.js
function main(): void {
  const target = { a: 1 };
  const result = Object.assign(target, { a: 2 }, { a: "c" });
  assertSameValue(result.a, "c", "The value should be c.");
}
