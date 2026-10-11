// test262: test/built-ins/String/prototype/codePointAt/returns-undefined-on-position-less-than-zero.js

function main(): void {
  assertSameValue('abc'.codePointAt(-1), undefined);
  assertSameValue('abc'.codePointAt(-Infinity), undefined);
}
