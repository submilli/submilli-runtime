// test262: test/built-ins/JSON/parse/15.12.1.1-g1-4.js

function main(): void {
  const n: number = JSON.parse(" 1234") as number;
  assertSameValue(n, 1234, "<SP> should be ignored");

  assertThrows((): void => {
    const m: number = JSON.parse("12 34") as number;
    assertSameValue(m, m, "unreachable");
  }, "<SP> should produce a syntax error as whitespace results in two tokens");
}
