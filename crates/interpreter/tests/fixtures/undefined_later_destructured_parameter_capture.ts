function objectPattern(read: () => number = () => later, { later = 42 }: { later?: number } = {}): number {
  return read();
}
function requiredPattern(read: () => number = () => later, { later }: { later: number }): number {
  return read();
}
function arrayPattern(read: () => number = () => later, [later = 42]: [number?] = []): number {
  return read();
}
function main(): void {
  assert(objectPattern() === 42, "default closure sees later initialized destructured binding");
  assert(objectPattern(undefined, { later: 7 }) === 7, "explicit property initializes shared binding");
  assert(requiredPattern(undefined, { later: 9 }) === 9, "required pattern initializes in declaration order");
  assert(arrayPattern() === 42, "later array pattern uses shared initialized binding");
}
