// test262: test/built-ins/encodeURIComponent/S15.1.3.4_A4_T4.js

function main(): void {
  assertSameValue(encodeURIComponent(""), "", "#1: \"\"");
  assertSameValue(encodeURIComponent("http://unipro.ru"), "http%3A%2F%2Funipro.ru", "#2: http://unipro.ru");
  assertSameValue(encodeURIComponent("http://www.google.ru/support/jobs/bin/static.py?page=why-ru.html&sid=liveandwork"), "http%3A%2F%2Fwww.google.ru%2Fsupport%2Fjobs%2Fbin%2Fstatic.py%3Fpage%3Dwhy-ru.html%26sid%3Dliveandwork", "#3: http://www.google.ru/support/jobs/bin/static.py?page=why-ru.html&sid=liveandwork");
  assertSameValue(encodeURIComponent("http://en.wikipedia.org/wiki/UTF-8#Description"), "http%3A%2F%2Fen.wikipedia.org%2Fwiki%2FUTF-8%23Description", "#4: http://en.wikipedia.org/wiki/UTF-8#Description");
}
