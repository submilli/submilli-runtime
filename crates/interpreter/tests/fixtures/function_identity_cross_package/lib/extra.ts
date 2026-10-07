export function dec(a: number): number {
  return a - 1;
}

export function getDec(): (a: number) => number {
  return dec;
}
