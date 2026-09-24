let current: number | null = 3;
function clear(): boolean { current = null; return false; }
function read(): number {
  if (current === null || clear()) return 9;
  return current;
}
function invoke(callback: () => number): number { return callback(); }
function main(): void {
  const callback = read;
  const actual: unknown = invoke(callback);
  assert(actual === null);
}
