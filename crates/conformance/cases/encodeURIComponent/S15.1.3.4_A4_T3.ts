// test262: test/built-ins/encodeURIComponent/S15.1.3.4_A4_T3.js
// The original `a !== X && a !== Y` throw guard becomes `assert(a === X || a === Y)`.

function main(): void {
  const r1 = encodeURIComponent("http://unipro.ru/\nabout");
  assert(r1 === "http%3A%2F%2Funipro.ru%2F%0Aabout" || r1 === "http%3A%2F%2Funipro.ru%2F%0aabout", "#1: http://unipro.ru/\\nabout");
  const r2 = encodeURIComponent("http://unipro.ru/\vabout");
  assert(r2 === "http%3A%2F%2Funipro.ru%2F%0Babout" || r2 === "http%3A%2F%2Funipro.ru%2F%0babout", "#2: http://unipro.ru/\\vabout");
  const r3 = encodeURIComponent("http://unipro.ru/\fabout");
  assert(r3 === "http%3A%2F%2Funipro.ru%2F%0Cabout" || r3 === "http%3A%2F%2Funipro.ru%2F%0cabout", "#3: http://unipro.ru/\\fabout");
  const r4 = encodeURIComponent("http://unipro.ru/\rabout");
  assert(r4 === "http%3A%2F%2Funipro.ru%2F%0Dabout" || r4 === "http%3A%2F%2Funipro.ru%2F%0dabout", "#4: http://unipro.ru/\\rabout");
}
