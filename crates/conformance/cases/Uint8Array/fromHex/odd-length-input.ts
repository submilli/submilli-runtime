// test262: test/built-ins/Uint8Array/fromHex/odd-length-input.js
// The runtime throws SyntaxError here; assertThrows matches it via the base Error.

function main(): void {
  assertThrows((): void => {
    Uint8Array.fromHex("a");
  }, "odd-length hex input");
}
