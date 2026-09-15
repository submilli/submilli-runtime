// test262: test/built-ins/JSON/parse/15.12.1.1-g2-2.js

function main(): void {
  assertThrows((): void => {
    const s: string = JSON.parse("'abc'") as string;
    assertSameValue(s, s, "unreachable");
  }, "a JSONString may not be delimited by single quotes");
}
