// test262: test/built-ins/encodeURI/S15.1.3.3_A4_T1.js

function main(): void {
  assertSameValue(encodeURI("http://unipro.ru/0123456789"), "http://unipro.ru/0123456789", "#1: http://unipro.ru/0123456789");
  assertSameValue(encodeURI("aAbBcCdDeEfFgGhHiIjJkKlLmMnNoOpPqQrRsStTuUvVwWxXyYzZ"), "aAbBcCdDeEfFgGhHiIjJkKlLmMnNoOpPqQrRsStTuUvVwWxXyYzZ", "#2: aAbBcCdDeEfFgGhHiIjJkKlLmMnNoOpPqQrRsStTuUvVwWxXyYzZ");
  assertSameValue(encodeURI("aA_bB-cC.dD!eE~fF*gG'hH(iI)jJ;kK/lL?mM:nN@oO&pP=qQ+rR$sS,tT9uU8vV7wW6xX5yY4zZ"), "aA_bB-cC.dD!eE~fF*gG'hH(iI)jJ;kK/lL?mM:nN@oO&pP=qQ+rR$sS,tT9uU8vV7wW6xX5yY4zZ", "#3: ");
}
