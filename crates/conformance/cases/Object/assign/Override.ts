// test262: test/built-ins/Object/assign/Override.js
// Adaptation: Object.keys replaces getOwnPropertyNames; all properties here are enumerable.
// String keys replace numeric index spelling.
function main(): void {
  const target = { a: 1 };
  const result = Object.assign(target, "1a2c3", { a: "c" }, undefined, { b: 6 }, null, 125, { a: 5 });
  assertSameValue(Object.keys(result).length, 7, "The length should be 7 in the final object.");
  assertSameValue(result.a, 5, "The value should be {a:5}.");
  assertSameValue(result["0"], "1", "first character");
  assertSameValue(result["1"], "a", "second character");
  assertSameValue(result["2"], "2", "third character");
  assertSameValue(result["3"], "c", "fourth character");
  assertSameValue(result["4"], "3", "fifth character");
  assertSameValue(result.b, 6, "The value should be {b:6}.");
}
