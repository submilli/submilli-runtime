function nothing(): void {}
function main(): void {
  const pair: [number, void] = [1, nothing()];
  assert(pair[0] === 1, "tuple retains required number");
  assert(pair[1] === undefined, "tuple retains void value");
}
