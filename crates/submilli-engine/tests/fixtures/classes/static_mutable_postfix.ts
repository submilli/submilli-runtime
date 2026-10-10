class A {
  static x: number = 1;
}

class B extends A {}

class Counter {
  static count: number = 0;
  static total: bigint = 0n;
}

function main(): void {
  Counter.count++;
  assert(Counter.count === 1);

  const old: number = Counter.count++;
  assert(old === 1);
  assert(Counter.count === 2);

  Counter.count--;
  assert(Counter.count === 1);

  Counter.total++;
  assert(Counter.total === 1n);

  // Through a subclass name: the definer's slot.
  B.x++;
  assert(A.x === 2);
}
