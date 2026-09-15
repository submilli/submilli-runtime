// test262: test/built-ins/Uint8Array/fromBase64/illegal-characters.js
// The runtime throws SyntaxError here; assertThrows matches it via the base Error.

function main(): void {
  const illegal: string[] = [
    "Zm.9v",
    "Zm9v^",
    "Zg==&",
    "Z\u{2212}==", // minus sign
    "Z\u{FF0B}==", // fullwidth plus sign
    "Zg\u{00A0}==", // nbsp
    "Zg\u{2009}==", // thin space
    "Zg\u{2028}==", // line separator
  ];
  for (const value of illegal) {
    assertThrows((): void => {
      Uint8Array.fromBase64(value);
    }, `fromBase64 #${value.length}`);
  }
}
