// `toJson` and `toString` may be optional, in an interface or a type literal.
// When the object leaves one out, it converts as if it declared neither:
// `JSON.stringify` serializes its fields and `String(x)` gives
// `[object Object]`. When present, the conversion calls it.
interface Labelled {
  a: number;
  toJson?: () => string;
  toString?: () => string;
}

function main(): void {
  const plain: Labelled = { a: 1 };
  assert(JSON.stringify(plain) === '{"a":1}', "an absent toJson");
  assert(JSON.stringify([plain]) === '[{"a":1}]', "an absent toJson in an array");
  assert(String(plain) === "[object Object]", "an absent toString");

  const custom: Labelled = { a: 2, toJson: () => '"two"', toString: () => "two" };
  assert(JSON.stringify(custom) === '"two"', "a present toJson");
  assert(JSON.stringify({ k: custom }) === '{"k":"two"}', "a nested present toJson");
  assert(String(custom) === "two", "a present toString");

  const literal: { toJson?: () => string; toString?: () => string; y: string } = { y: "q" };
  assert(JSON.stringify(literal) === '{"y":"q"}', "a type literal's absent toJson");
  assert(String(literal) === "[object Object]", "a type literal's absent toString");
}
