// test262: test/built-ins/RegExp/S15.10.2.6_A3_T1.js
// RegExpExecArray shape adapted to RegExpMatch (.match/.index/.input/.groups).

function main(): void {
  const m = /\bp/.exec("pilot\nsoviet robot\topenoffice");
  assert(m !== null, "exec must match");
  if (m === null) {
    return;
  }
  const matched = m.match;
  const index = m.index;
  const input = m.input;
  const captures = m.groups.length;
  assertSameValue(matched, "p", "\\b matches at a word boundary");
  assertSameValue(index, 0, "match offset");
  assertSameValue(input, "pilot\nsoviet robot\topenoffice", "input round-trips");
  assertSameValue(captures, 0, "no capture groups");
}
