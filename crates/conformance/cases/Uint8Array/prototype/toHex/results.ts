// test262: test/built-ins/Uint8Array/prototype/toHex/results.js

function main(): void {
  assertSameValue(new Uint8Array([]).toHex(), "");
  assertSameValue(new Uint8Array([102]).toHex(), "66");
  assertSameValue(new Uint8Array([102, 111]).toHex(), "666f");
  assertSameValue(new Uint8Array([102, 111, 111]).toHex(), "666f6f");
  assertSameValue(new Uint8Array([102, 111, 111, 98]).toHex(), "666f6f62");
  assertSameValue(new Uint8Array([102, 111, 111, 98, 97]).toHex(), "666f6f6261");
  assertSameValue(new Uint8Array([102, 111, 111, 98, 97, 114]).toHex(), "666f6f626172");
}
