// test262: test/built-ins/parseInt/S15.1.2.2_A2_T1.js

function main(): void {
  assertSameValue(parseInt("\u00091"), parseInt("1"), 'parseInt("\\u00091") must return the same value returned by parseInt("1")');

  assertSameValue(parseInt("		-1"), parseInt("-1"), 'parseInt("\\u0009\\u0009-1") must return the same value returned by parseInt("-1")');

  assertSameValue(parseInt("\t1"), parseInt("1"), 'parseInt(tab 1) must return the same value returned by parseInt("1")');

  assertSameValue(parseInt("\t\t\t1"), parseInt("1"), 'parseInt(tabs 1) must return the same value returned by parseInt("1")');

  assertSameValue(
    parseInt("\t\t\t	\t\t\t	-1"),
    parseInt("-1"),
    'parseInt(tabs -1) must return the same value returned by parseInt("-1")',
  );

  assertSameValue(parseInt("	"), NaN, 'parseInt("\\u0009") must return NaN');
}
