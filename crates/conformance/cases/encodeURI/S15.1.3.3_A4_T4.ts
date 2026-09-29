// test262: test/built-ins/encodeURI/S15.1.3.3_A4_T4.js

function main(): void {
  assertSameValue(encodeURI(""), "", "#1: \"\"");
  assertSameValue(encodeURI("http://unipro.ru"), "http://unipro.ru", "#2: http://unipro.ru");
  assertSameValue(encodeURI("http://www.google.ru/support/jobs/bin/static.py?page=why-ru.html&sid=liveandwork"), "http://www.google.ru/support/jobs/bin/static.py?page=why-ru.html&sid=liveandwork", "#3: http://www.google.ru/support/jobs/bin/static.py?page=why-ru.html&sid=liveandwork");
  assertSameValue(encodeURI("http://en.wikipedia.org/wiki/UTF-8#Description"), "http://en.wikipedia.org/wiki/UTF-8#Description", "#4: http://en.wikipedia.org/wiki/UTF-8#Description");
}
