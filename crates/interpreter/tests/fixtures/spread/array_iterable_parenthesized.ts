function main(): void {
  const direct: ((n: number) => number)[] = [...[(n) => n + 1]];
  const parenthesized: ((n: number) => number)[] = [...(([(n) => n + 1]))];
  assert(direct[0](2) === 3, "direct array context");
  assert(parenthesized[0](2) === 3, "parenthesized array keeps the same context");
  const explicit: ((n: number) => number)[] = [...(([(n: number) => n + 1]))];
  assert(explicit[0](2) === 3, "explicit callback parameter");
  assert([...((new Set<number>([4])))][0] === 4, "parenthesized iterable");
}
