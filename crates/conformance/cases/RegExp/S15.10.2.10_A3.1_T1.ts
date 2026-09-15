// test262: test/built-ins/RegExp/S15.10.2.10_A3.1_T1.js
// The original's Latin-1 row table, spot-checked; Test262Error throws become asserts.

function main(): void {
  const x00 = /\x00/.exec("\u0000");
  assert(x00 !== null, "\\x00 matches U+0000");
  const x01 = /\x01/.exec("\u0001");
  assert(x01 !== null, "\\x01 matches U+0001");
  const x0a = /\x0A/.exec("\u000A");
  assert(x0a !== null, "\\x0A matches U+000A");
  const xff = /\xFF/.exec("\u00FF");
  assert(xff !== null, "\\xFF matches U+00FF");
  if (xff === null) {
    return;
  }
  const matched = xff.match;
  assertSameValue(matched, "\u00FF", "match text round-trips");
}
