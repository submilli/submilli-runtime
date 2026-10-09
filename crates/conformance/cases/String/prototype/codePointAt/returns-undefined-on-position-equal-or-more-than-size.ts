// test262: test/built-ins/String/prototype/codePointAt/returns-undefined-on-position-equal-or-more-than-size.js

function main(): void {
  assertSameValue('abc'.codePointAt(3), undefined);
  assertSameValue('abc'.codePointAt(4), undefined);
  assertSameValue('abc'.codePointAt(Infinity), undefined);
}
