// expect-error: expected `1`, got `number`
// expect-error-count: 1
// A label of an earlier clause that updates the discriminant (`x++`) leaves a
// later clause's match saying nothing about the value `x` holds.
function tag(x: number): number {
  return 99;
}
function f(t: 1 | 2): number {
  let x: number = t;
  switch (x) {
    case tag(x++):
      return 0;
    case 1: {
      const one: 1 = x;
      return one;
    }
  }
  return -1;
}
function main(): void {
  f(1);
}
