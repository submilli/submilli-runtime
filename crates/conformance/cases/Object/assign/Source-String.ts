// test262: test/built-ins/Object/assign/Source-String.js
// Adaptation: {} replaces new Object(); string keys replace numeric index spelling.
function main(): void {
  const result = Object.assign({}, "123");
  assertSameValue(result["0"], "1", "The value of result[0] is expected to be 1");
  assertSameValue(result["1"], "2", "The value of result[1] is expected to be 2");
  assertSameValue(result["2"], "3", "The value of result[2] is expected to be 3");
}
