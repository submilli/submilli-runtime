// Map/Set with class-instance keys: hash (vtable slot 3) is FNV over the data
// fields, consistent with the nominal equals (slot 2) — field-equal same-class
// keys collide and dedup, a subclass never collides with its base even with
// equal fields, nullable fields hash as 0, and lookups survive rehash growth.
class Key {
  a: number;
  b: string;

  constructor(a: number, b: string) {
    this.a = a;
    this.b = b;
  }
}

class SubKey extends Key {
  c: number;

  constructor(a: number, b: string, c: number) {
    super(a, b);
    this.c = c;
  }
}

class NullableKey {
  v: number | null;

  constructor(v: number | null) {
    this.v = v;
  }
}

function main(): void {
  const m: Map<Key, number> = new Map<Key, number>();
  m.set(new Key(1, "x"), 10);
  m.set(new Key(2, "y"), 20);
  assert(m.size === 2);
  assert(m.get(new Key(1, "x")) === 10);
  assert(m.get(new Key(2, "y")) === 20);
  assert(m.get(new Key(1, "y")) === undefined);
  assert(m.has(new Key(1, "x")));
  assert(!m.has(new Key(3, "z")));

  m.set(new Key(1, "x"), 11);
  assert(m.size === 2);
  assert(m.get(new Key(1, "x")) === 11);

  const mixed: Map<Key, number> = new Map<Key, number>();
  const subAsKey: Key = new SubKey(1, "x", 0);
  mixed.set(new Key(1, "x"), 1);
  mixed.set(subAsKey, 2);
  assert(mixed.size === 2);
  assert(mixed.get(new Key(1, "x")) === 1);
  const probe: Key = new SubKey(1, "x", 0);
  assert(mixed.get(probe) === 2);

  const nm: Map<NullableKey, string> = new Map<NullableKey, string>();
  nm.set(new NullableKey(null), "none");
  nm.set(new NullableKey(0), "zero");
  assert(nm.get(new NullableKey(null)) === "none");
  assert(nm.get(new NullableKey(0)) === "zero");

  const s: Set<Key> = new Set<Key>();
  for (let i = 0; i < 40; i++) {
    s.add(new Key(i % 20, "k"));
  }
  assert(s.size === 20);
  assert(s.has(new Key(0, "k")));
  assert(s.has(new Key(19, "k")));
  assert(!s.has(new Key(20, "k")));
}
