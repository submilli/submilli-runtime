// test262: test/built-ins/RegExp/S15.10.2.10_A4.1_T1.js
// The original's row table, spot-checked via the dynamic constructor so the
// \uHHHH pattern escapes stay visible; Test262Error throws become asserts.

function main(): void {
  const u0 = new RegExp("\\u0000", "").exec("\u0000");
  assert(u0 !== null, "\\u0000 matches U+0000");
  const u1 = new RegExp("\\u0001", "").exec("\u0001");
  assert(u1 !== null, "\\u0001 matches U+0001");
  const ua = new RegExp("\\u000A", "").exec("\u000A");
  assert(ua !== null, "\\u000A matches U+000A");
  const uff = new RegExp("\\u00FF", "").exec("\u00FF");
  assert(uff !== null, "\\u00FF matches U+00FF");
  if (uff === null) {
    return;
  }
  const matched = uff.match;
  assertSameValue(matched, "\u00FF", "match text round-trips");
}
