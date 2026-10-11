// `toJson` and `toString` may be optional, in an interface or a type literal.
// When the object leaves one out, it converts as if it declared neither:
// `JSON.stringify` serializes its fields and `String(x)` gives
// `[object Object]`. When present, the conversion calls it.
function viaParam(x: Labelled): string {
  return `${x}`;
}

function genericText<T>(x: T): string {
  return `${x}`;
}

class Shelf<T> {
  v: T;
  constructor(v: T) {
    this.v = v;
  }
  show(): string {
    return `[${this.v}]`;
  }
}

function everyText<T>(xs: T[]): string {
  let text = "";
  for (const x of xs) text += `${x};`;
  return text;
}

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

  assert(viaParam(plain) === "[object Object]", "interpolating an absent toString");
  assert(viaParam(custom) === "two", "interpolating a present toString");
  const holder: { v: Labelled } = { v: plain };
  assert(`${holder.v}` === "[object Object]", "interpolating a field with an absent toString");
  assert([plain, custom].join("|") === "[object Object]|two", "join mixes absent and present toString");
  assert(JSON.stringify([custom, plain]) === '["two",{"a":1}]', "an array mixes absent and present toJson");
  const maybe: Labelled | null = plain;
  assert(JSON.stringify(maybe) === '{"a":1}', "a nullable union with an absent toJson");

  const nested: Labelled[][] = [[plain, { a: 2, toString: () => "b" }], []];
  assert(nested.join(";") === "[object Object],b;", "joining nested arrays with an absent toString");
  const later: Labelled = { a: 3 };
  later.toString = () => "later";
  later.toJson = () => '"lj"';
  assert(`${later}` === "later" && JSON.stringify([later]) === '["lj"]', "an optional conversion assigned later");
  assert(genericText(literal) === "[object Object]", "a generic type literal with an absent toString");
  const byKey = new Map<string, Labelled>([["k", plain]]);
  assert(`${byKey.get("k")!}` === "[object Object]", "a map value with an absent toString");
  const either: Labelled | { y: string; toString?: () => string } = literal;
  assert(`${either}` === "[object Object]", "a union of object types with absent toString");

  // A generic or `unknown` value holding null interpolates as "null".
  assert(genericText<Labelled | null>(null) === "null", "interpolating a generic null");
  const nothing: unknown = null;
  assert(`${nothing}` === "null", "interpolating an unknown null");
  const unknowns: unknown[] = [null];
  const box: { v: unknown } = { v: null };
  assert(`${unknowns[0]}` === "null" && `${box.v}` === "null", "interpolating an unknown null read from a container");
  assert(new Shelf<number | null>(null).show() === "[null]", "interpolating a generic field holding null");
  assert(everyText<string | null>(["a", null]) === "a;null;", "interpolating generic elements holding null");
}
