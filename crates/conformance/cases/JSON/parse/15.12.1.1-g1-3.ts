// test262: test/built-ins/JSON/parse/15.12.1.1-g1-3.js

function main(): void {
  const n: number = JSON.parse("\n1234") as number;
  assertSameValue(n, 1234, "<LF> should be ignored");

  assertThrows((): void => {
    const m: number = JSON.parse("12\n34") as number;
    assertSameValue(m, m, "unreachable");
  }, "<LF> should produce a syntax error as whitespace results in two tokens");
}
