// `?.` on a getter-backed class property. Accessors back no payload slot, so
// the chain emitter must route to the synthetic getter rather than the dynamic
// field-name scan (which would find nothing and read a null).
class Computed {
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

function size_of(c: Computed | null): number | null {
  return c?.size;
}

function main(): void {
  const c = new Computed(4);
  assert(size_of(c) === 8, "getter through optional chain");
  assert(size_of(null) === null, "short-circuit on null receiver");

  c.size = 20;
  assert(size_of(c) === 20, "getter reflects the setter");
}
