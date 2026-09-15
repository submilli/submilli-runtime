// Accessors satisfy an interface property through a synthetic `get <p>` /
// `set <p>` payload entry, so an inherited accessor has to be installed on the
// subclass's instances the same way an inherited method is.
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

class Sub extends Computed {}

function read(s: Sized): number {
  return s.size;
}

function main(): void {
  const sub = new Sub(4);
  assert(read(sub) === 8, "inherited getter through an interface");

  const s: Sized = sub;
  s.size = 20;
  assert(read(sub) === 20, "inherited setter through an interface");
  assert(sub.size === 20, "and through the class-typed receiver");
}
