// Two type params, methods returning a differently-instantiated self type.
class Pair<K, V> {
  constructor(
    readonly k: K,
    readonly v: V,
  ) {}
  swap(): Pair<V, K> {
    return new Pair(this.v, this.k);
  }
}

function main(): void {
  const p = new Pair("key", 9);
  assert(p.k === "key", "readonly ctor property K");
  assert(p.v === 9, "readonly ctor property V");
  const s = p.swap();
  assert(s.k === 9, "swapped K");
  assert(s.v === "key", "swapped V");
  const back = s.swap();
  assert(back.k === "key" && back.v === 9, "double swap");
}
