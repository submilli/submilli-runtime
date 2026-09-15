// test262: test/built-ins/JSON/parse/15.12.1.1-g2-1.js

function main(): void {
  const s: string = JSON.parse("\"abc\"") as string;
  assertSameValue(s, "abc", "JSONStrings can be written using double quotes");
}
