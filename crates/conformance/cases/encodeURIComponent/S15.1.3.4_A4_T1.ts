// test262: test/built-ins/encodeURIComponent/S15.1.3.4_A4_T1.js

function main(): void {
  assertSameValue(encodeURIComponent("http://unipro.ru/0123456789"), "http%3A%2F%2Funipro.ru%2F0123456789", "#1: http://unipro.ru/0123456789");
  assertSameValue(encodeURIComponent("aAbBcCdDeEfFgGhHiIjJkKlLmMnNoOpPqQrRsStTuUvVwWxXyYzZ"), "aAbBcCdDeEfFgGhHiIjJkKlLmMnNoOpPqQrRsStTuUvVwWxXyYzZ", "#2: aAbBcCdDeEfFgGhHiIjJkKlLmMnNoOpPqQrRsStTuUvVwWxXyYzZ");
  assertSameValue(encodeURIComponent(";/?:@&=+$,"), "%3B%2F%3F%3A%40%26%3D%2B%24%2C", "#3: ");
}
