function read(bytes: Uint8Array | null, index: number): number | null { return bytes?.[index]; }
function main(): void {
  const bytes = new Uint8Array([1]);
  for (const index of [-1, 0.5, 1, NaN, Infinity]) {
    let failed = false;
    try { read(bytes, index); } catch (error) { failed = error instanceof RangeError; }
    assert(failed, "catchable byte bounds error");
    assert(read(null, index) === null, "null short circuit before bounds");
  }
  let failed = false;
  try { const old = bytes[1]++; } catch (error) { failed = error instanceof RangeError; }
  assert(failed && bytes[0] === 1, "postfix bounds and unchanged bytes");
}
