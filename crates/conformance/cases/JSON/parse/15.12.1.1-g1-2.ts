// test262: test/built-ins/JSON/parse/15.12.1.1-g1-2.js

function main(): void {
  const n: number = JSON.parse("\r1234") as number;
  assertSameValue(n, 1234, "<cr> should be ignored");

  assertThrows((): void => {
    const m: number = JSON.parse("12\r34") as number;
    assertSameValue(m, m, "unreachable");
  }, "<CR> should produce a syntax error as whitespace results in two tokens");
}
