// test262: test/built-ins/decodeURIComponent/S15.1.3.2_A4_T4.js

function main(): void {
  assertSameValue(decodeURIComponent(""), "", "#1: \"\"");
  assertSameValue(decodeURIComponent("http://unipro.ru"), "http://unipro.ru", "#2: http://unipro.ru");
  assertSameValue(decodeURIComponent("http:%2f%2Fwww.google.ru/support/jobs/bin/static.py%3Fpage%3dwhy-ru.html%26sid%3Dliveandwork"), "http://www.google.ru/support/jobs/bin/static.py?page=why-ru.html&sid=liveandwork", "#3: http:%2f%2Fwww.google.ru/support/jobs/bin/static.py%3Fpage3dwhy-ru.html%26sid3Dliveandwork");
  assertSameValue(decodeURIComponent("http:%2F%2Fen.wikipedia.org/wiki/UTF-8%23Description"), "http://en.wikipedia.org/wiki/UTF-8#Description", "#4: http:%2F%2Fen.wikipedia.org/wiki/UTF-8%23Description");
}
