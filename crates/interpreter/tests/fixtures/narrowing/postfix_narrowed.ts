let global: number | null = 4;
class Counter { static value: bigint | null = 4n; }
function main(): void {
  let captured: number | null = 7;
  const read = (): number | null => captured;
  if (captured !== null) {
    assert(captured++ === 7);
    assert(read() === 8, "postfix updates captured storage");
    captured--;
    assert(read() === 7);
  }
  let capturedBig: bigint | null = 7n;
  const readBig = (): bigint | null => capturedBig;
  if (capturedBig !== null) {
    assert(capturedBig++ === 7n);
    capturedBig--;
    assert(readBig() === 7n);
  }

  let n: number | null = 1;
  if (n !== null) {
    assert(n++ === 1);
    n++;
    assert(n-- === 3);
    assert(n === 2);
  }
  let b: bigint | null = 1n;
  if (b !== null) { assert(b++ === 1n); b--; assert(b === 1n); }
  if (global !== null) { assert(global++ === 4); global--; assert(global === 4); }
  if (Counter.value !== null) { assert(Counter.value++ === 4n); Counter.value--; assert(Counter.value === 4n); }
}
