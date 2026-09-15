// test262: test/built-ins/JSON/parse/15.12.1.1-g5-1.js

function main(): void {
  const s: string = JSON.parse("\"\\u0058\"") as string;
  assertSameValue(s, "X", "Unicode escape sequences are allowed in a JSONString");
}
