function main(): void {
  const m = new Map<number | null, number>();
  const s = new Set<number | null>();
  assert(!m.has(null));
  assert(!s.has(null));
  m.set(null, 1);
  m.set(null, 2);
  s.add(null);
  s.add(null);
  assert(m.size === 1 && s.size === 1);
  for (let i = 0; i < 40; i++) { m.set(i, i); s.add(i); }
  assert(m.get(null) === 2 && s.has(null));
  assert(Array.from(m.keys())[0] === null);
  assert(Array.from(s)[0] === null);
  let seen = 0;
  m.forEach((v, k) => { if (k === null) { assert(v === 2); seen++; } });
  s.forEach((v) => { if (v === null) seen++; });
  assert(seen === 2);
  assert(m.delete(null) && s.delete(null));
  assert(!m.delete(null) && !s.delete(null));
  m.set(null, 3); s.add(null);
  assert(Array.from(m.keys())[40] === null);
  assert(Array.from(s)[40] === null);
  const copy = new Map(m);
  const setCopy = new Set(s);
  assert(copy.get(null) === 3 && setCopy.has(null));
  m.clear(); s.clear();
  assert(!m.has(null) && !s.has(null));
}
