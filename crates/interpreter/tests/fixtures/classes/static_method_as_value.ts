class Maker {
  static make(n: number): number {
    return n * 3;
  }
}

function apply(g: (n: number) => number, v: number): number {
  return g(v);
}

function main(): void {
  const f = Maker.make;
  assert(f(4) === 12);
  assert(apply(Maker.make, 5) === 15);
}
