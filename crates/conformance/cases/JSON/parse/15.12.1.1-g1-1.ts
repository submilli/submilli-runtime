// test262: test/built-ins/JSON/parse/15.12.1.1-g1-1.js

function main(): void {
  const n: number = JSON.parse("\t1234") as number;
  assertSameValue(n, 1234, "<TAB> should be ignored");

  assertThrows((): void => {
    const m: number = JSON.parse("12\t34") as number;
    assertSameValue(m, m, "unreachable");
  }, "<TAB> should produce a syntax error as whitespace results in two tokens");
}
