// test262: test/built-ins/JSON/parse/15.12.1.1-0-2.js

function main(): void {
  assertThrows((): void => {
    const n: number = JSON.parse("\u000b1234") as number;
    assertSameValue(n, n, "unreachable");
  }, "<VT> is not valid JSON whitespace");
}
