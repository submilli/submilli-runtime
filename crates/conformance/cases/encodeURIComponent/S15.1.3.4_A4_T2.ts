// test262: test/built-ins/encodeURIComponent/S15.1.3.4_A4_T2.js
// The original `a !== X && b !== Y` throw guard becomes `assert(a === X || b === Y)`.
// CHECK#3's lowercase alternative encodes a different input ("...%23..."), as upstream does.

function main(): void {
  const r1a = encodeURIComponent("http://ru.wikipedia.org/wiki/Юникод");
  const r1b = encodeURIComponent("http://ru.wikipedia.org/wiki/Юникод");
  assert(r1a === "http%3A%2F%2Fru.wikipedia.org%2Fwiki%2F%D0%AE%D0%BD%D0%B8%D0%BA%D0%BE%D0%B4" || r1b === "http%3A%2F%2Fru.wikipedia.org%2Fwiki%2F" + "%D0%AE%D0%BD%D0%B8%D0%BA%D0%BE%D0%B4".toLowerCase(), "#1: http://ru.wikipedia.org/wiki/Юникод");
  const r2a = encodeURIComponent("http://ru.wikipedia.org/wiki/Юникод#Ссылки");
  const r2b = encodeURIComponent("http://ru.wikipedia.org/wiki/Юникод#Ссылки");
  assert(r2a === "http%3A%2F%2Fru.wikipedia.org%2Fwiki%2F%D0%AE%D0%BD%D0%B8%D0%BA%D0%BE%D0%B4%23%D0%A1%D1%81%D1%8B%D0%BB%D0%BA%D0%B8" || r2b === "http%3A%2F%2Fru.wikipedia.org%2Fwiki%2F" + "%D0%AE%D0%BD%D0%B8%D0%BA%D0%BE%D0%B4%23%D0%A1%D1%81%D1%8B%D0%BB%D0%BA%D0%B8".toLowerCase(), "#2: http://ru.wikipedia.org/wiki/Юникод#Ссылки");
  const r3a = encodeURIComponent("http://ru.wikipedia.org/wiki/Юникод#Версии Юникода");
  const r3b = encodeURIComponent("http://ru.wikipedia.org/wiki/Юникод%23Версии Юникода");
  assert(r3a === "http%3A%2F%2Fru.wikipedia.org%2Fwiki%2F%D0%AE%D0%BD%D0%B8%D0%BA%D0%BE%D0%B4%23%D0%92%D0%B5%D1%80%D1%81%D0%B8%D0%B8%20%D0%AE%D0%BD%D0%B8%D0%BA%D0%BE%D0%B4%D0%B0" || r3b === "http%3A%2F%2Fru.wikipedia.org%2Fwiki%2F" + "%D0%AE%D0%BD%D0%B8%D0%BA%D0%BE%D0%B4#%D0%92%D0%B5%D1%80%D1%81%D0%B8%D0%B8%20%D0%AE%D0%BD%D0%B8%D0%BA%D0%BE%D0%B4%D0%B0".toLowerCase(), "#3: http://ru.wikipedia.org/wiki/Юникод#Версии Юникода");
}
