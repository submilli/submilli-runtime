// test262: test/built-ins/String/prototype/codePointAt/returns-undefined-on-position-less-than-zero.js
// `undefined` is spelled `null`.

function main(): void {
  assertSameValue("abc".codePointAt(-1), null, "-1 is before the start");
  assertSameValue("abc".codePointAt(-Infinity), null, "-Infinity is before the start");
}
