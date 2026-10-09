// test262: test/built-ins/String/prototype/codePointAt/returns-undefined-on-position-equal-or-more-than-size.js
// `undefined` is spelled `null`.

function main(): void {
  assertSameValue("abc".codePointAt(3), null, "position 3 is past the end");
  assertSameValue("abc".codePointAt(4), null, "position 4 is past the end");
  assertSameValue("abc".codePointAt(Infinity), null, "Infinity is past the end");
}
