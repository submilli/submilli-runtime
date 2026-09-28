function main(): string {
  const counts: Record<string, number> = {};
  const key: string = "hello";
  assert(counts[key] === null);
  counts[key] = (counts[key] ?? 0) + 1;
  assert(counts[key] === 1);
  counts.other = 2;
  assert(counts.other === 2);
  assert(key in counts);
  assert(!("missing" in counts));
  const copy = counts;
  copy[key] = 3;
  assert(counts[key] === 3);
  const finite: Record<"a" | "b", number> = { a: 1, b: 2 };
  assert(finite.a + finite.b === 3);
  const signature: { [key: string]: number } = counts;
  assert(signature[key] === 3);
  const computed: Record<string, number> = { [key]: 4, other: 5 };
  assert(computed[key] === 4);
  assert(Object.keys(computed).length === 2);
  const parsed = JSON.parse('{"x": 3}') as Record<string, number>;
  assert(parsed.x === 3);
  let failed = false;
  try { const bad = JSON.parse('{"x": "wrong"}') as Record<string, number>; } catch (e: Error) { failed = true; }
  assert(failed);
  const roundtrip = JSON.parse(JSON.stringify(counts)) as Record<string, number>;
  assert(roundtrip[key] === 3);
  assert(roundtrip.other === 2);
  return JSON.stringify(computed);
}
