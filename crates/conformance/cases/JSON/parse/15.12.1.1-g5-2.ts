// test262: test/built-ins/JSON/parse/15.12.1.1-g5-2.js

function main(): void {
  assertThrows((): void => {
    const s: string = JSON.parse("\"\\u005\"") as string;
    assertSameValue(s, s, "unreachable");
  }, "a JSONString UnicodeEscape may not have fewer than 4 hex characters");
}
