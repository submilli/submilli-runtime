function pick({ value }: { value: number } = { value: 4 }, next: number = value + 1): number {
  return next;
}
function main(): void {
  assert(pick() === 5, "earlier destructuring completes before later defaults");
  assert(pick({ value: 8 }) === 9, "explicit earlier argument is destructured before later defaults");
}
