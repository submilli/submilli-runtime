// A byte read as a chain's last step gives `undefined` for an index that finds
// no byte, as in JavaScript: the chain's result already has room for it.
function read(bytes: Uint8Array | null, index: number): number | undefined { return bytes?.[index]; }
function main(): void {
  const bytes = new Uint8Array([1]);
  for (const index of [-1, 0.5, 1, NaN, Infinity]) {
    assert(read(bytes, index) === undefined, "a missing byte reads as undefined");
    assert(read(null, index) === undefined, "null short circuit before bounds");
  }
  assert(read(bytes, 0) === 1, "an in-range byte");
  let failed = false;
  try { const old = bytes[1]++; } catch (error) { failed = error instanceof MissingValueError; }
  assert(failed && bytes[0] === 1, "postfix bounds and unchanged bytes");
}
