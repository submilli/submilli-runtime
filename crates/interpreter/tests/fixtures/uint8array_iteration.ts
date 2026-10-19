// SUB-1757, SUB-1510: a `Uint8Array` iterates its bytes, as in tsc and
// JavaScript: `for-of`, `keys`, `values`, `entries` and `Array.from`.
function main(): void {
  const bytes = new Uint8Array(3);
  bytes[1] = 7;
  let sum = 0;
  for (const b of bytes) {
    sum += b;
  }
  assert(sum === 7);

  const indices: number[] = [];
  for (const i of bytes.keys()) {
    indices.push(i);
  }
  assert(indices.join(",") === "0,1,2");

  const seen: number[] = [];
  for (const v of bytes.values()) {
    seen.push(v);
    // The iterator is live: a write ahead of it is seen.
    if (seen.length === 1) bytes[2] = 9;
  }
  assert(seen.join(",") === "0,7,9");

  const pairs: string[] = [];
  for (const [i, b] of bytes.entries()) {
    pairs.push(`${i}:${b}`);
  }
  assert(pairs.join(",") === "0:0,1:7,2:9");

  assert(Array.from(bytes).join(",") === "0,7,9");
  assert(Array.from(bytes, (b: number): number => b * 2).join(",") === "0,14,18");

  const it = bytes.values();
  it.next(); it.next(); it.next();
  assert(it.next().done === true && it.next().done === true);
}
