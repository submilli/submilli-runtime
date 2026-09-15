// Divergence vector, not a test262 port. JSON.stringify emits object keys
// in lexicographic order at every nesting level (RFC 8785 canonical form),
// independent of construction order; JS enumerates in insertion order
// (see rejected/JSON/stringify/property-order.js).

function main(): void {
  assertSameValue(JSON.stringify({ b: 1, a: 2, c: 3 }), "{\"a\":2,\"b\":1,\"c\":3}");
  assertSameValue(
    JSON.stringify({ z: { d: 1, c: 2 }, a: [{ b: 1, a: 0 }] }),
    "{\"a\":[{\"a\":0,\"b\":1}],\"z\":{\"c\":2,\"d\":1}}",
    "nested objects sort too",
  );
}
