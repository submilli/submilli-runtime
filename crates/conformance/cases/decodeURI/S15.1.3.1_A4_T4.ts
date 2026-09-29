// test262: test/built-ins/decodeURI/S15.1.3.1_A4_T4.js

function main(): void {
  assertSameValue(decodeURI(""), "", "#1: \"\"");
  assertSameValue(decodeURI("http:%2f%2Funipro.ru"), "http:%2f%2Funipro.ru", "#2: http:%2f%2Funipro.ru");
  assertSameValue(decodeURI("http://www.google.ru/support/jobs/bin/static.py%3Fpage%3dwhy-ru.html%26sid%3Dliveandwork"), "http://www.google.ru/support/jobs/bin/static.py%3Fpage%3dwhy-ru.html%26sid%3Dliveandwork", "#3: http://www.google.ru/support/jobs/bin/static.py%3Fpage%3dwhy-ru.html%26sid%3Dliveandwork");
  assertSameValue(decodeURI("http://en.wikipedia.org/wiki/UTF-8%23Description"), "http://en.wikipedia.org/wiki/UTF-8%23Description", "%234: http://en.wikipedia.org/wiki/UTF-8%23Description");
}
