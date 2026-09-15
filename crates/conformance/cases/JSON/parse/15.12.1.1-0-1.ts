// test262: test/built-ins/JSON/parse/15.12.1.1-0-1.js

function main(): void {
  assertThrows((): void => {
    const n: number = JSON.parse("12\t\r\n 34") as number;
    assertSameValue(n, n, "unreachable");
  }, "whitespace results in two tokens");
}
