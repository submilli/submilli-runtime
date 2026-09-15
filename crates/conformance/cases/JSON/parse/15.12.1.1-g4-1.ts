// test262: test/built-ins/JSON/parse/15.12.1.1-g4-1.js

function main(): void {
  assertThrows((): void => {
    const s: string = JSON.parse("\"\u0000\u0001\u0002\u0003\u0004\u0005\u0006\u0007\"") as string;
    assertSameValue(s, s, "unreachable");
  }, "raw U+0000..U+0007 are not valid JSONStringCharacters");
}
