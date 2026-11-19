function main(): number {
  function defaulted(value: number = 2): number {
    const read = (): number => value * 3;
    return read();
  }

  function optional(value?: number): number {
    const read = (): number => value === undefined ? 0 : value;
    return read();
  }

  function union(value: number | string): number {
    const read = (): number => typeof value === "string" ? 6 : value;
    return read();
  }

  if (defaulted() !== 6 || defaulted(1) !== 3) return 1;
  if (optional() !== 0 || optional(4) !== 4) return 2;
  if (union("a") !== 6 || union(5) !== 5) return 3;
  return 0;
}
