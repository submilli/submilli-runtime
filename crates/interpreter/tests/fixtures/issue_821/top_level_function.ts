function main(): void {
  let calls = 0;
  const f = (): number => { calls++; return 1; };
  const erased: unknown = f;
  assert(JSON.stringify(f) === undefined);
  assert(JSON.stringify(erased) === undefined);
  assert(JSON.stringify([f]) === '[null]');
  assert(JSON.stringify({ f }) === '{}');
  assert(calls === 0);
  // A function field, not an iterator: built-in iterators have no own keys (SUB-2280).
  const holder: unknown = { next: (): number => 1 };
  const methods = Object.values(holder);
  assert(methods.length > 0);
  for (const method of methods) {
    assert(JSON.stringify(method) === undefined);
  }
}
