// Bindings destructured from a parameter are mutable, as in TypeScript.
function bump({ count }: { count: number }): number {
  count = count + 1;
  return count;
}

function shift([first, second]: [number, number]): number {
  first++;
  second *= 2;
  return first + second;
}

function main(): void {
  assert(bump({ count: 1 }) === 2, "an object pattern parameter binding");
  assert(shift([1, 2]) === 6, "array pattern parameter bindings");
}
