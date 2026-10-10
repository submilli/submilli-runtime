let current: number | null = 3;
function clear(): boolean { current = null; return false; }
function read(): number {
  if (current === null || clear()) return 9;
  return current;
}
function main(): void {
  const values = [1].map((_value: number): number => read());
  const actual: unknown = values[0];
  assert(actual === null);
}
