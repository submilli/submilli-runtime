// test262: test/built-ins/String/prototype/replaceAll/replaceValue-value-replaces-string.js

function main(): void {
  const result = "aaab a a aac".replaceAll("aa", "z");
  assertSameValue(result, "zab a a zc");
}
