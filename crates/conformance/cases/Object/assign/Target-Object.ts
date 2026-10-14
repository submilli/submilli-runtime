// test262: test/built-ins/Object/assign/Target-Object.js
function main(): void {
  const target = { foo: 1 };
  const result = Object.assign(target, { a: 2 });
  assertSameValue(result.foo, 1, "The value should be {foo: 1}.");
  assertSameValue(result.a, 2, "The value should be {a: 2}.");
}
