// test262: test/built-ins/decodeURI/S15.1.3.1_A4_T1.js

function main(): void {
  assertSameValue(decodeURI("http://unipro.ru/0123456789"), "http://unipro.ru/0123456789", "#1: http://unipro.ru/0123456789");
  assertSameValue(decodeURI("%41%42%43%44%45%46%47%48%49%4A%4B%4C%4D%4E%4F%50%51%52%53%54%55%56%57%58%59%5A"), "ABCDEFGHIJKLMNOPQRSTUVWXYZ", "#2: ABCDEFGHIJKLMNOPQRSTUVWXYZ");
  assertSameValue(decodeURI("%61%62%63%64%65%66%67%68%69%6A%6B%6C%6D%6E%6F%70%71%72%73%74%75%76%77%78%79%7A"), "abcdefghijklmnopqrstuvwxyz", "#3: abcdefghijklmnopqrstuvwxyz");
}
