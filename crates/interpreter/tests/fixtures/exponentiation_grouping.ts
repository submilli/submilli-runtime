function main(): void {
  const x: number = 3;
  assert((-x) ** 2 === 9, "parenthesized base");
  assert(-(x ** 2) === -9, "parenthesized power");
  assert(2 ** -2 === 0.25, "negative exponent");
  assert(((-x) as number) ** 2 === 9, "cast with explicit grouping");
  assert(2 ** 3 ** 2 === 512, "right associative");
}
