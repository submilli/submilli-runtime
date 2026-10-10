// An interface may declare `toString` as an optional property. When the
// object leaves it out, `String(x)` falls back to `[object Object]`, as in
// JavaScript; when it is present, `String(x)` calls it.
interface Labeled {
  x: number;
  toString?: () => string;
}

function main(): void {
  const plain: Labeled = { x: 1 };
  const custom: Labeled = { x: 2, toString: () => "custom" };
  assert(String(plain) === "[object Object]", "an absent toString");
  assert(String(custom) === "custom", "a present toString");
}
