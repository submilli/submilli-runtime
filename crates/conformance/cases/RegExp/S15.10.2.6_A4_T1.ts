// test262: test/built-ins/RegExp/S15.10.2.6_A4_T1.js
// RegExpExecArray shape adapted to RegExpMatch (.match/.index/.input/.groups).

function main(): void {
  const m = /\Bevil\B/.exec("devils arise\tfor\nevil");
  assert(m !== null, "exec must match");
  if (m === null) {
    return;
  }
  const matched = m.match;
  const index = m.index;
  const input = m.input;
  assertSameValue(matched, "evil", "\\B matches inside a word");
  assertSameValue(index, 1, "match offset");
  assertSameValue(input, "devils arise\tfor\nevil", "input round-trips");
}
