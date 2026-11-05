function main(): void {
  type Key = `id_${string}`;
  type Suffix = "a" | "b";
  type FiniteKey = `n_${Suffix}`;
  interface Flags { [key: Key]: boolean; }
  const flags: Flags = {};
  flags.id_x = true;
  const values: { [key: Key]: number } = {};
  values.id_x = 2;
  const finite: Record<FiniteKey, number> = { n_a: 1, n_b: 2 };
  const source: unknown = { id_x: true, other: "text" };
  const checked = source as Flags;
  assert(flags.id_x === true && values.id_x === 2);
  assert(finite.n_b === 2 && checked.id_x === true);
}
