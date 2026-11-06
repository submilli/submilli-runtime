// test262: test/built-ins/RegExp/S15.10.2.6_A4_T1.js

function main(): void {
  const m = /\Bevil\B/.exec("devils arise\tfor\nevil");
  assert(m !== null, "exec must match");
  if (m === null) {
    return;
  }
  const matched = m[0];
  const index = m.index;
  const input = m.input;
  assertSameValue(matched, "evil", "\\B matches inside a word");
  assertSameValue(index, 1, "match offset");
  assertSameValue(input, "devils arise\tfor\nevil", "input round-trips");
}
