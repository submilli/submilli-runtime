let calls: number = 0;

function tick(): number {
  calls = calls + 1;
  return calls;
}

const first: number = tick();

class Counter {
  static count: number = 0;
  static readonly LIMIT: number = 10;
  static order: number = tick();
}

const last: number = tick();

function main(): void {
  assert(Counter.count === 0);
  assert(Counter.LIMIT === 10);
  // Statics initialize in source order, interleaved with module `let`/`const`.
  assert(first === 1);
  assert(Counter.order === 2);
  assert(last === 3);
}
