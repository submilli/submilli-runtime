function main(): void {
  let calls = 0;
  const f = (): number => { calls++; return 1; };
  const erased: unknown = f;
  assert(JSON.stringify(f) === "null");
  assert(JSON.stringify(erased) === "null");
  assert(JSON.stringify([f]) === '[null]');
  assert(JSON.stringify({ f }) === '{}');
  assert(calls === 0);
  const iterator = new Set<number>([1]).values();
  const methods = Object.values(iterator);
  assert(methods.length > 0);
  for (const method of methods) {
    assert(JSON.stringify(method) === "null");
  }
}
