function first<T>(xs: T[]): T {
  return xs[0];
}

function main(): void {
  const opt: { a: number; b?: string | null } = { a: 1, b: null };
  const values = Object.values(opt);
  assert(values.length === 2, "two fields");
  assert(values[0] as number === 1, "set field survives");
  assert(values[1] === null, "present optional field holds null unknown");

  const entries = Object.entries(opt);
  assert(entries[1][1] === null, "entries value slot holds null");

  const xs: Array<string | null> = [null, "a"];
  assert(first(xs) === null, "erased generic returns null element");
}
