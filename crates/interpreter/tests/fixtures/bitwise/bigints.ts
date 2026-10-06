function main(): void {
  assert((13n & 7n) === 5n && (8n | 3n) === 11n && (13n ^ 7n) === 10n, "logic");
  assert(~0n === -1n && ~-1n === 0n, "complement");
  assert((-9n >> 2n) === -3n, "negative right shift rounds down");
  assert((8n << -1n) === 4n && (8n >> -1n) === 16n, "negative count");
  assert((1n << 100n) === 1267650600228229401496703205376n, "multi limb");
  assert(((1n << 100n) | 7n) === 1267650600228229401496703205383n, "multi limb logic");
  const huge = 100000000000000000000000000000000000000n;
  assert((1n >> huge) === 0n && (-1n >> huge) === -1n, "huge shrinking shift");
  assert((0n << huge) === 0n, "huge zero shift");
  let refused = false;
  try { const result = 1n << huge; assert(result === 0n, "unreachable"); }
  catch (error) { refused = error instanceof RangeError; }
  assert(refused, "huge expanding shift bounded");
  let bounded = false;
  try { const result = 1n << 4194304n; assert(result === 0n, "unreachable"); }
  catch (error) { bounded = error instanceof RangeError; }
  assert(bounded, "allocation ceiling");
}
