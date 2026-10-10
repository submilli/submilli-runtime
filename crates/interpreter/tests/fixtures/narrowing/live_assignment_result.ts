let current: number | null = 3;
function clear(): boolean { current = null; return false; }
function assigned(): number {
  let saved = 1;
  if (current === null || clear()) { return 0; }
  return (saved = current);
}
function main(): void {
  const result: unknown = assigned();
  assert(result === null);
}
