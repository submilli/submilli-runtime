// test262: test/built-ins/parseFloat/S15.1.2.3_A2_T1.js

function main(): void {
  assertSameValue(parseFloat("	1.1"), parseFloat("1.1"), 'parseFloat("\\u00091.1") must equal parseFloat("1.1")');
  assertSameValue(parseFloat("		-1.1"), parseFloat("-1.1"), 'parseFloat("\\u0009\\u0009-1.1") must equal parseFloat("-1.1")');
  assertSameValue(parseFloat("\t1.1"), parseFloat("1.1"), "parseFloat(tab 1.1) must equal parseFloat(1.1)");
  assertSameValue(parseFloat("\t\t\t1.1"), parseFloat("1.1"), "parseFloat(tabs 1.1) must equal parseFloat(1.1)");
  assertSameValue(parseFloat("\t\t\t	\t\t\t	-1.1"), parseFloat("-1.1"), "parseFloat(tabs -1.1) must equal parseFloat(-1.1)");
  assertSameValue(parseFloat("	"), NaN, 'parseFloat("\\u0009") must return NaN');
}
