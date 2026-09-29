// test262: test/built-ins/decodeURI/S15.1.3.1_A4_T3.js

function main(): void {
  assertSameValue(decodeURI("http://unipro.ru/%0Aabout"), "http://unipro.ru/\nabout", "#1: http://unipro.ru/%A0about");
  assertSameValue(decodeURI("http://unipro.ru/%0Babout"), "http://unipro.ru/\vabout", "#2: http://unipro.ru/%0Babout");
  assertSameValue(decodeURI("http://unipro.ru/%0Cabout"), "http://unipro.ru/\fabout", "#3: http://unipro.ru/%0Cabout");
  assertSameValue(decodeURI("http://unipro.ru/%0Dabout"), "http://unipro.ru/\rabout", "#4: http://unipro.ru/%0Dabout");
}
