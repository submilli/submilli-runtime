function truthy(n: number): boolean {
  return !!n;
}

function main(): void {
  assert(!truthy(0), "0 is falsy");
  assert(!truthy(-0), "-0 is falsy");
  assert(!truthy(NaN), "NaN is falsy");
  assert(truthy(1), "1 is truthy");
  assert(truthy(-1), "-1 is truthy");
  assert(truthy(0.5), "fractions are truthy");
  if (0) {
    assert(false, "if (0) must not enter");
  }
  const x: string = 1 ? "a" : "b";
  assert(x === "a", "ternary on nonzero number");
  const y: string = 0 ? "a" : "b";
  assert(y === "b", "ternary on zero");
}
