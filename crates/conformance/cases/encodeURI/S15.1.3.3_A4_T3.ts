// test262: test/built-ins/encodeURI/S15.1.3.3_A4_T3.js
// The original `a !== X && a !== Y` throw guard becomes `assert(a === X || a === Y)`.

function main(): void {
  const r1 = encodeURI("http://unipro.ru/\nabout");
  assert(r1 === "http://unipro.ru/%0Aabout" || r1 === "http://unipro.ru/%0aabout", "#1: http://unipro.ru/\\nabout");
  const r2 = encodeURI("http://unipro.ru/\vabout");
  assert(r2 === "http://unipro.ru/%0Babout" || r2 === "http://unipro.ru/%0babout", "#2: http://unipro.ru/\\vabout");
  const r3 = encodeURI("http://unipro.ru/\fabout");
  assert(r3 === "http://unipro.ru/%0Cabout" || r3 === "http://unipro.ru/%0cabout", "#3: http://unipro.ru/\\fabout");
  const r4 = encodeURI("http://unipro.ru/\rabout");
  assert(r4 === "http://unipro.ru/%0Dabout" || r4 === "http://unipro.ru/%0dabout", "#4: http://unipro.ru/\\rabout");
}
