let current: number | null = 3;
function clear(): boolean { current = null; return false; }
function read(): number {
  if (current === null || clear()) return 9;
  return current;
}
function previous(value: number): number { return value++; }
function main(): void { assert(previous(read()) === 0); }
