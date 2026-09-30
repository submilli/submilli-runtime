// test262: test/built-ins/decodeURIComponent/S15.1.3.2_A4_T3.js

function main(): void {
  assertSameValue(decodeURIComponent("http://unipro.ru/%0Aabout"), "http://unipro.ru/\nabout", "#1: http://unipro.ru/%A0about");
  assertSameValue(decodeURIComponent("http://unipro.ru/%0Babout"), "http://unipro.ru/\vabout", "#2: http://unipro.ru/%0Babout");
  assertSameValue(decodeURIComponent("http://unipro.ru/%0Cabout"), "http://unipro.ru/\fabout", "#3: http://unipro.ru/%0Cabout");
  assertSameValue(decodeURIComponent("http://unipro.ru/%0Dabout"), "http://unipro.ru/\rabout", "#4: http://unipro.ru/%0Dabout");
}
