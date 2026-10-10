let value: number | null = 1;
function calculate(): number {
  if (value !== null) return value ** Infinity;
  return 0;
}
function main(): void { assert(Number.isNaN(calculate())); }
