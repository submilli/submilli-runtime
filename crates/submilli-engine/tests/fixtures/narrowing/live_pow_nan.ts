let value: number | null = 1;
function calculate(): number {
  if (value !== null) return value ** NaN;
  return 0;
}
function main(): void { assert(Number.isNaN(calculate())); }
