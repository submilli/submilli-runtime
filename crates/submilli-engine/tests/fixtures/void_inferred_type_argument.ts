function nothing(): void {}
function main(): void {
  const values = [1, 2].map((value: number) => value === 1 ? nothing() : 1);
  assert(values[0] === undefined, "map infers void in element union");
  assert(values[1] === 1, "map preserves the defined branch");
}
