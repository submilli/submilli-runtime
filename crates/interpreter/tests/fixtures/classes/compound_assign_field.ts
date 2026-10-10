// Read-modify-write on class fields: `+=` and `++` on data fields (own,
// inherited, private), on an accessor pair, and on a `readonly` field inside
// the declaring constructor.
class Counter {
  private hits: number = 0;
  count: number = 0;
  label: string = "";
  total: bigint = 0n;
  readonly base: number;

  constructor(base: number) {
    this.base = base;
    this.base += 1;
  }

  get value(): number {
    return this.hits;
  }

  set value(v: number) {
    this.hits = v;
  }

  bump(): void {
    this.count += 1;
    this.count++;
    this.hits -= 2;
    this.label += "x";
    this.total += 10n;
    this.value += 5;
  }
}

class Loud extends Counter {
  shout(): void {
    this.count *= 3;
    this.label += "!";
  }
}

function main(): void {
  const c = new Counter(1);
  assert(c.base === 2);
  c.bump();
  assert(c.count === 2);
  assert(c.label === "x");
  assert(c.total === 10n);
  assert(c.value === 3);

  c.count += 10;
  c.count--;
  c.value += 1;
  assert(c.count === 11);
  assert(c.value === 4);

  const l = new Loud(0);
  l.bump();
  l.shout();
  assert(l.count === 6);
  assert(l.label === "x!");
}
