let sourceCalls: number = 0;
let defaultCalls: number = 0;

function source(): { absent?: number | null; present: number; nullable: number | null } {
  sourceCalls = sourceCalls + 1;
  return { present: 7, nullable: null };
}

function fallback(): number {
  defaultCalls = defaultCalls + 1;
  return 5;
}

function main(): void {
  const { absent = fallback(), present: renamed = fallback(), nullable = fallback() } = source();
  assert(absent === 5, "missing property uses its default");
  assert(renamed === 7, "present renamed property skips its default");
  assert(nullable === null, "null does not trigger a default");
  assert(sourceCalls === 1, "source evaluates once");
  assert(defaultCalls === 1, "only the missing property evaluates its default");
  const values: (number | null | undefined)[] = [9, undefined, null];
  const [, missing = fallback(), keptNull = fallback()] = values;
  assert(missing === 5, "array default observes the index after a hole");
  assert(keptNull === null, "array default preserves null");
  assert(defaultCalls === 2, "array fallback is lazy");
}
