// test262: test/built-ins/RegExp/S15.10.2.6_A3_T1.js

function main(): void {
  const m = /\bp/.exec("pilot\nsoviet robot\topenoffice");
  assert(m !== null, "exec must match");
  if (m === null) {
    return;
  }
  const matched = m[0];
  const index = m.index;
  const input = m.input;
  const captures = m.length - 1;
  assertSameValue(matched, "p", "\\b matches at a word boundary");
  assertSameValue(index, 0, "match offset");
  assertSameValue(input, "pilot\nsoviet robot\topenoffice", "input round-trips");
  assertSameValue(captures, 0, "no capture groups");
}
