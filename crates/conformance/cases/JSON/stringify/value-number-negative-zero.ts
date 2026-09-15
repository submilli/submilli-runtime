// test262: test/built-ins/JSON/stringify/value-number-negative-zero.js
// The original's heterogeneous array ['-0', 0, -0] is split by element type.

function main(): void {
  assertSameValue(JSON.stringify(-0), "0");
  assertSameValue(JSON.stringify(["-0"]), "[\"-0\"]");
  assertSameValue(JSON.stringify([0, -0]), "[0,0]");
  assertSameValue(JSON.stringify({ key: -0 }), "{\"key\":0}");
}
