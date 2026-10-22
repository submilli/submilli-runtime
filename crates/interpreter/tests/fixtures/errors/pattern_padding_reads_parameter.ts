// expect-error: field `z` does not exist on `{ x?: number; y?: number }`
// A property default that reads a name of its own pattern cannot be typed
// before the pattern binds, so the object default is not padded with it.
// tsc rejects it too, as an implicit `any` (TS7022).
function readsSibling({ x = 3, y = 1, z = x } = {}): number {
  return x + y + z;
}

function main(): void {
  console.log(readsSibling({ y: 2 }));
}
