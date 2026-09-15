// test262: test/built-ins/Uint8Array/fromBase64/last-chunk-invalid.js
// Default-mode subset: the lastChunkHandling variants are recorded in
// skips/Uint8Array.md. The runtime throws SyntaxError; assertThrows matches
// it via the base Error.

function main(): void {
  // Non-padded incomplete chunk 'A'
  assertThrows((): void => {
    Uint8Array.fromBase64("A");
  }, "incomplete chunk 'A'");

  // Non-padded incomplete chunk 'ABCDA'
  assertThrows((): void => {
    Uint8Array.fromBase64("ABCDA");
  }, "incomplete chunk 'ABCDA'");

  // Incomplete padding in chunk 'AA='
  assertThrows((): void => {
    Uint8Array.fromBase64("AA=");
  }, "incomplete padding 'AA='");

  // Padded chunks always throw when incomplete before padding
  const illegal: string[] = [
    "=",
    "==",
    "===",
    "====",
    "=====",
    "A=",
    "A==",
    "A===",
    "A====",
    "A=====",
    "AA====",
    "AA=====",
    "AAA==",
    "AAA===",
    "AAA====",
    "AAA=====",
    "AAAA=",
    "AAAA==",
    "AAAA===",
    "AAAA====",
    "AAAA=====",
    "AAAAA=",
    "AAAAA==",
    "AAAAA===",
    "AAAAA====",
    "AAAAA=====",
  ];
  for (const value of illegal) {
    assertThrows((): void => {
      Uint8Array.fromBase64(value);
    }, `fromBase64('${value}')`);
  }
}
