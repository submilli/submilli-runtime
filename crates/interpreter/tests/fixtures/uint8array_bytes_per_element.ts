// SUB-1824: BYTES_PER_ELEMENT is 1 on Uint8Array and on its instances, as in
// tsc's lib and JavaScript.
function main(): void {
  assert(Uint8Array.BYTES_PER_ELEMENT === 1);
  const bytes = new Uint8Array([1, 2, 3]);
  assert(bytes.BYTES_PER_ELEMENT === 1);
  assert(bytes.length * bytes.BYTES_PER_ELEMENT === bytes.byteLength);
}
