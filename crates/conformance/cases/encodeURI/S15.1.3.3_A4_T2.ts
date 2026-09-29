// test262: test/built-ins/encodeURI/S15.1.3.3_A4_T2.js
// The original `a !== X && b !== Y` throw guard becomes `assert(a === X || b === Y)`.

function main(): void {
  const r1a = encodeURI("http://ru.wikipedia.org/wiki/Юникод");
  const r1b = encodeURI("http://ru.wikipedia.org/wiki/Юникод");
  assert(r1a === "http://ru.wikipedia.org/wiki/%D0%AE%D0%BD%D0%B8%D0%BA%D0%BE%D0%B4" || r1b === "http://ru.wikipedia.org/wiki/" + "%D0%AE%D0%BD%D0%B8%D0%BA%D0%BE%D0%B4".toLowerCase(), "#1: http://ru.wikipedia.org/wiki/Юникод");
  const r2a = encodeURI("http://ru.wikipedia.org/wiki/Юникод#Ссылки");
  const r2b = encodeURI("http://ru.wikipedia.org/wiki/Юникод#Ссылки");
  assert(r2a === "http://ru.wikipedia.org/wiki/%D0%AE%D0%BD%D0%B8%D0%BA%D0%BE%D0%B4#%D0%A1%D1%81%D1%8B%D0%BB%D0%BA%D0%B8" || r2b === "http://ru.wikipedia.org/wiki/" + "%D0%AE%D0%BD%D0%B8%D0%BA%D0%BE%D0%B4#%D0%A1%D1%81%D1%8B%D0%BB%D0%BA%D0%B8".toLowerCase(), "#2: http://ru.wikipedia.org/wiki/Юникод#Ссылки");
  const r3a = encodeURI("http://ru.wikipedia.org/wiki/Юникод#Версии Юникода");
  const r3b = encodeURI("http://ru.wikipedia.org/wiki/Юникод#Версии Юникода");
  assert(r3a === "http://ru.wikipedia.org/wiki/%D0%AE%D0%BD%D0%B8%D0%BA%D0%BE%D0%B4#%D0%92%D0%B5%D1%80%D1%81%D0%B8%D0%B8%20%D0%AE%D0%BD%D0%B8%D0%BA%D0%BE%D0%B4%D0%B0" || r3b === "http://ru.wikipedia.org/wiki/" + "%D0%AE%D0%BD%D0%B8%D0%BA%D0%BE%D0%B4#%D0%92%D0%B5%D1%80%D1%81%D0%B8%D0%B8%20%D0%AE%D0%BD%D0%B8%D0%BA%D0%BE%D0%B4%D0%B0".toLowerCase(), "#3: http://ru.wikipedia.org/wiki/Юникод#Версии Юникода");
}
