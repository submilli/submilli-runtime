// expect-error: expected `Pair<number, number>`
class Pair<K, V> {
  constructor(
    readonly k: K,
    readonly v: V,
  ) {}
}

function takeNumPair(p: Pair<number, number>): number {
  return p.k;
}

function main(): void {
  const p = new Pair(1, "a");
  assert(takeNumPair(p) === 1);
}
