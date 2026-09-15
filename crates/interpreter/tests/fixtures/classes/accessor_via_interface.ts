// An accessor property satisfies an interface, and access through an
// interface-typed receiver dispatches to the accessor at runtime. A sibling
// class backs the same property with a plain field, proving per-instance
// dispatch through the same interface.
interface Sized {
  size: number;
}

class Computed implements Sized {
  private base: number;
  constructor(base: number) {
    this.base = base;
  }
  get size(): number {
    return this.base * 2;
  }
  set size(v: number) {
    this.base = v / 2;
  }
}

class Stored implements Sized {
  size: number;
  constructor(size: number) {
    this.size = size;
  }
}

function readSize(s: Sized): number {
  return s.size;
}

function bump(s: Sized): void {
  s.size = 100;
}

function main(): void {
  const c: Sized = new Computed(5);
  const s: Sized = new Stored(7);

  // Accessor-backed and field-backed, both read through the interface.
  assert(readSize(c) === 10);
  assert(readSize(s) === 7);

  // Writes through the interface: accessor setter vs plain field.
  bump(c);
  bump(s);
  assert(readSize(c) === 100);
  assert(readSize(s) === 100);
}
