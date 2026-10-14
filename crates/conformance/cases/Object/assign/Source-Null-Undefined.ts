// test262: test/built-ins/Object/assign/Source-Null-Undefined.js
// Adaptation: {} replaces new Object(); Object.is uses structural equality here.
function main(): void {
  const target = {};
  const result = Object.assign(target, undefined, null);
  assertSameValue(result, target, "The value of result is expected to equal the value of target");
}
