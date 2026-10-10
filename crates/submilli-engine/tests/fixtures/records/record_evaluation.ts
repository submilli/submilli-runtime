function nullable(value: Record<string, number> | null): Record<string, number> | null { return value; }
function main(): void {
  let calls = 0;
  const next = (): string => { calls += 1; return "x"; };
  const record = { [next()]: 1, [next()]: 2 };
  assert(calls === 2);
  assert(record.x === 2);
  const finite: Record<"x", number> = { x: 1 };
  const key: "x" = "x";
  finite[key] += 2;
  assert(finite[key]++ === 3);
  assert(finite.x === 4);
  const mixed: { x: number; [key: string]: number } = { x: 1 };
  mixed[key] += 2;
  assert(mixed[key]++ === 3);
  assert(mixed.x === 4);
  const maybe: Record<string, number> | null = nullable(record);
  assert(maybe?.[next()] === 2);
  assert(calls === 3);
  assert(maybe?.x === 2);
  const missing: Record<string, number> | null = nullable(null);
  assert(missing?.[next()] === undefined);
  assert(calls === 3);
  const make = (key: string): () => Record<string, number> => (): Record<string, number> => ({ [key]: 8 });
  assert(make("captured")().captured === 8);
}
