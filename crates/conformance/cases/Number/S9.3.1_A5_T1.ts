// test262: test/built-ins/Number/S9.3.1_A5_T1.js

function main(): void {
  assertSameValue(Number("-0"), -0);
  assertSameValue(Number("-Infinity"), -Infinity);
  assertSameValue(Number("-1234567890"), -1234567890);
  assertSameValue(Number("-1234.5678"), -1234.5678);
  assertSameValue(Number("-1234.5678e90"), -1234.5678e90);
  assertSameValue(Number("-1234.5678E90"), -1234.5678E90);
  assertSameValue(Number("-1234.5678e-90"), -1234.5678e-90);
  assertSameValue(Number("-1234.5678E-90"), -1234.5678E-90);
}
